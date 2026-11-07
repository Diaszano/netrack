//! Parallel HTTP throughput engine and latency statistics.
//!
//! The engine is transport-agnostic: callers supply a [`StreamFactory`] that
//! performs one HTTP transfer and adds bytes to a shared counter as they flow.
//! The engine runs that factory in parallel loops, ramps the number of streams
//! during warm-up while throughput keeps growing, and reports the rate measured
//! between the end of warm-up and the end of the test.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::{StreamExt, stream};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior};

/// Size of [`UPLOAD_CHUNK`].
const UPLOAD_CHUNK_LEN: usize = 1024 * 1024;
/// Window over which the instantaneous rate is computed during ramp-up.
const RATE_WINDOW: Duration = Duration::from_secs(1);
/// Pause before a stream retries after a failed transfer.
const RETRY_DELAY: Duration = Duration::from_millis(100);
/// Minimum window-over-window growth that triggers adding streams.
const RAMP_GROWTH: f64 = 1.10;

/// 1 MiB of random bytes, generated once. Upload bodies repeat clones of it
/// (reference-counted, no copies).
pub static UPLOAD_CHUNK: LazyLock<Bytes> = LazyLock::new(|| {
    let mut buf = vec![0u8; UPLOAD_CHUNK_LEN];
    rand::fill(&mut buf[..]);
    Bytes::from(buf)
});

#[derive(Debug, thiserror::Error)]
pub enum MeasureError {
    #[error("all streams failed: {0}")]
    AllStreamsFailed(String),
    #[error("transfer stalled")]
    Stalled,
    #[error("http error: {0}")]
    Http(String),
}

/// One HTTP transfer, adding transferred bytes to the counter as they flow.
/// The engine re-invokes it in a loop per stream until time is up.
pub type StreamFactory =
    Arc<dyn Fn(Arc<AtomicU64>) -> BoxFuture<'static, Result<(), MeasureError>> + Send + Sync>;

#[derive(Debug, Clone)]
pub struct ThroughputConfig {
    pub duration: Duration,
    pub warmup: Duration,
    pub initial_streams: usize,
    pub max_streams: usize,
    pub step: usize,
    pub sample_every: Duration,
    pub stall_timeout: Duration,
}

impl Default for ThroughputConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(10),
            warmup: Duration::from_secs(2),
            initial_streams: 4,
            max_streams: 32,
            step: 4,
            sample_every: Duration::from_millis(250),
            stall_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LatencyStats {
    pub ping: f64,
    pub jitter: f64,
}

/// Ping is the median sample; jitter is the mean absolute difference between
/// consecutive samples. `None` with fewer than 2 samples.
pub fn latency_stats(samples_ms: &[f64]) -> Option<LatencyStats> {
    if samples_ms.len() < 2 {
        return None;
    }
    let mut sorted = samples_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    let ping = if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    };
    let diffs: f64 = samples_ms.windows(2).map(|w| (w[1] - w[0]).abs()).sum();
    let jitter = diffs / (samples_ms.len() - 1) as f64;
    Some(LatencyStats { ping, jitter })
}

/// Reads a response body chunk by chunk, adding each chunk's length to
/// `counter` without buffering the body. Non-2xx statuses are errors.
pub async fn drain_counting(
    resp: reqwest::Response,
    counter: &AtomicU64,
) -> Result<(), MeasureError> {
    let resp = resp
        .error_for_status()
        .map_err(|e| MeasureError::Http(e.to_string()))?;
    let mut body = resp.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| MeasureError::Http(e.to_string()))?;
        counter.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }
    Ok(())
}

/// A streaming request body of `total` bytes built from clones of
/// [`UPLOAD_CHUNK`], adding each chunk's length to `counter` as it is sent.
pub fn upload_body(total: u64, counter: Arc<AtomicU64>) -> reqwest::Body {
    let chunks = stream::unfold(total, move |left| {
        let counter = Arc::clone(&counter);
        async move {
            if left == 0 {
                return None;
            }
            let n = left.min(UPLOAD_CHUNK_LEN as u64) as usize;
            let chunk = if n == UPLOAD_CHUNK_LEN {
                UPLOAD_CHUNK.clone()
            } else {
                UPLOAD_CHUNK.slice(..n)
            };
            counter.fetch_add(n as u64, Ordering::Relaxed);
            Some((Ok::<_, std::io::Error>(chunk), left - n as u64))
        }
    });
    reqwest::Body::wrap_stream(chunks)
}

/// State shared between the engine and its stream tasks.
#[derive(Default)]
struct Shared {
    bytes: Arc<AtomicU64>,
    /// Streams that have failed at least once.
    failed_streams: AtomicUsize,
    last_error: Mutex<Option<String>>,
}

impl Shared {
    fn last_error(&self) -> String {
        self.last_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| "unknown error".to_string())
    }
}

fn spawn_streams(
    tasks: &mut JoinSet<()>,
    count: usize,
    factory: &StreamFactory,
    shared: &Arc<Shared>,
) {
    for _ in 0..count {
        let factory = Arc::clone(factory);
        let shared = Arc::clone(shared);
        tasks.spawn(async move {
            let mut has_failed = false;
            loop {
                if let Err(err) = factory(Arc::clone(&shared.bytes)).await {
                    if !has_failed {
                        has_failed = true;
                        shared.failed_streams.fetch_add(1, Ordering::Relaxed);
                    }
                    *shared.last_error.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(err.to_string());
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        });
    }
}

/// Runs parallel transfers for `cfg.duration` and returns the rate in Mbps
/// (2 decimals) measured after `cfg.warmup`. All stream tasks are aborted and
/// joined before returning.
pub async fn measure_throughput(
    cfg: &ThroughputConfig,
    factory: StreamFactory,
) -> Result<f64, MeasureError> {
    let shared = Arc::new(Shared::default());
    let mut tasks = JoinSet::new();
    let mut streams = cfg.initial_streams.min(cfg.max_streams);
    spawn_streams(&mut tasks, streams, &factory, &shared);

    let start = Instant::now();
    let warmup_at = start + cfg.warmup;
    let end_at = start + cfg.duration;
    let mut ticker = tokio::time::interval_at(start + cfg.sample_every, cfg.sample_every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    let mut window: VecDeque<(Instant, u64)> = VecDeque::from([(start, 0)]);
    let mut prev_rate = 0.0;
    let mut last_bytes = 0;
    let mut last_progress = start;
    let mut warm: Option<(Instant, u64)> = None;

    let result = loop {
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(end_at) => {
                let (t1, bytes1) = (Instant::now(), shared.bytes.load(Ordering::Relaxed));
                let (t0, bytes0) = warm.unwrap_or((start, 0));
                if bytes1 == bytes0 && shared.failed_streams.load(Ordering::Relaxed) >= streams {
                    break Err(MeasureError::AllStreamsFailed(shared.last_error()));
                }
                let mbps = (bytes1 - bytes0) as f64 * 8.0 / (t1 - t0).as_secs_f64() / 1e6;
                break Ok((mbps * 100.0).round() / 100.0);
            }
            _ = tokio::time::sleep_until(warmup_at), if warm.is_none() => {
                warm = Some((Instant::now(), shared.bytes.load(Ordering::Relaxed)));
            }
            _ = ticker.tick() => {
                let now = Instant::now();
                let bytes = shared.bytes.load(Ordering::Relaxed);

                if bytes != last_bytes {
                    last_bytes = bytes;
                    last_progress = now;
                } else if now - last_progress >= cfg.stall_timeout {
                    // A stall caused by every stream erroring is reported with
                    // the underlying error, which is more useful than "stalled".
                    if shared.failed_streams.load(Ordering::Relaxed) >= streams {
                        break Err(MeasureError::AllStreamsFailed(shared.last_error()));
                    }
                    break Err(MeasureError::Stalled);
                }

                window.push_back((now, bytes));
                while window.len() > 1 && now - window[1].0 >= RATE_WINDOW {
                    window.pop_front();
                }
                let (t_old, b_old) = window[0];
                let elapsed = (now - t_old).as_secs_f64();
                let rate = if elapsed > 0.0 { (bytes - b_old) as f64 / elapsed } else { 0.0 };

                if now < warmup_at && rate > prev_rate * RAMP_GROWTH && streams < cfg.max_streams {
                    let add = cfg.step.min(cfg.max_streams - streams);
                    spawn_streams(&mut tasks, add, &factory, &shared);
                    streams += add;
                }
                prev_rate = rate;
            }
        }
    };

    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    result
}
