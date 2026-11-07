//! Speed test providers: a common trait plus native Cloudflare and LibreSpeed
//! implementations built on the throughput engine.

pub mod cloudflare;
pub mod librespeed;

use std::net::IpAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::{Duration, Instant};

use futures::FutureExt;
use futures::future::BoxFuture;
use reqwest::header::HeaderMap;

use crate::db::config::{AppConfig, Provider};
use crate::measure::{
    LatencyStats, MeasureError, StreamFactory, ThroughputConfig, drain_counting, latency_stats,
    measure_throughput, upload_body,
};

pub use cloudflare::Cloudflare;
pub use librespeed::{LibreServer, LibreSource, LibreSpeed};

/// Hard bound on one whole provider run.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);
/// Bound on each latency probe request.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Sequential latency samples taken after the warm-up request.
const LATENCY_SAMPLES: usize = 20;
/// Bytes per upload request; the engine re-invokes the factory as needed.
const UPLOAD_BYTES: u64 = 25_000_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    pub ping: f64,
    pub jitter: f64,
    pub download: f64,
    pub upload: f64,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
    pub server_host: Option<String>,
    pub result_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Network(String),
    #[error("no usable server")]
    NoServer,
    #[error(transparent)]
    Measure(#[from] MeasureError),
}

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub local_addr: Option<IpAddr>,
    pub throughput: ThroughputConfig,
}

pub trait SpeedProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn run<'a>(&'a self, opts: &'a RunOptions)
    -> BoxFuture<'a, Result<Measurement, ProviderError>>;
}

/// Builds the provider selected by `cfg`. HTTP clients are created per run
/// from `RunOptions::local_addr`.
pub fn build_provider(cfg: &AppConfig) -> Box<dyn SpeedProvider> {
    match cfg.provider {
        Provider::Cloudflare => Box::new(Cloudflare::new()),
        Provider::LibreSpeed => {
            let list_url = librespeed::DEFAULT_LIST_URL.to_string();
            let source = match (&cfg.libre_url, cfg.libre_server_id) {
                (Some(url), _) => LibreSource::Custom(librespeed::custom_libre_server(url)),
                (None, Some(id)) => LibreSource::ById { list_url, id },
                (None, None) => LibreSource::Auto { list_url },
            };
            Box::new(LibreSpeed::new(source))
        }
    }
}

pub(crate) fn net_err(e: impl std::fmt::Display) -> ProviderError {
    ProviderError::Network(e.to_string())
}

/// Bounds a whole provider run.
pub(crate) async fn with_run_timeout<T>(
    fut: impl Future<Output = Result<T, ProviderError>>,
) -> Result<T, ProviderError> {
    tokio::time::timeout(RUN_TIMEOUT, fut)
        .await
        .unwrap_or_else(|_| Err(ProviderError::Network("timeout".into())))
}

/// One bounded GET whose body is read to the end (so the connection is reused).
pub(crate) async fn timed_get(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<(), ProviderError> {
    get_ok(client, url, timeout)
        .await?
        .bytes()
        .await
        .map_err(net_err)?;
    Ok(())
}

async fn get_ok(
    client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<reqwest::Response, ProviderError> {
    client
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(net_err)
}

/// Server processing time in ms from a `Server-Timing` header: the first
/// `dur=` parameter across all entries and header lines, as the official
/// Cloudflare speed test does.
pub(crate) fn server_timing_ms(headers: &HeaderMap) -> Option<f64> {
    headers
        .get_all("server-timing")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .flat_map(|entry| entry.split(';').skip(1))
        .find_map(|param| {
            param
                .trim()
                .strip_prefix("dur=")?
                .trim_matches('"')
                .parse::<f64>()
                .ok()
        })
}

/// One latency sample: time to response headers minus the server time the
/// response reports, so only network round trip remains.
async fn latency_sample(client: &reqwest::Client, url: &str) -> Result<f64, ProviderError> {
    let started = Instant::now();
    let resp = get_ok(client, url, PROBE_TIMEOUT).await?;
    let ttfb_ms = started.elapsed().as_secs_f64() * 1000.0;
    let server_ms = server_timing_ms(resp.headers()).unwrap_or(0.0);
    resp.bytes().await.map_err(net_err)?;
    Ok((ttfb_ms - server_ms).max(0.01))
}

/// Probes latency: one warm-up request, then [`LATENCY_SAMPLES`] sequential
/// ones on the same connection. A failed warm-up fails the run; failed
/// samples are skipped.
pub(crate) async fn probe_latency(
    client: &reqwest::Client,
    url: impl Fn() -> String,
) -> Result<LatencyStats, ProviderError> {
    timed_get(client, &url(), PROBE_TIMEOUT).await?;
    let mut samples = Vec::with_capacity(LATENCY_SAMPLES);
    for _ in 0..LATENCY_SAMPLES {
        if let Ok(ms) = latency_sample(client, &url()).await {
            samples.push(ms);
        }
    }
    latency_stats(&samples)
        .ok_or_else(|| ProviderError::Network("not enough latency samples".into()))
}

fn download_factory(client: &reqwest::Client, url: String) -> StreamFactory {
    let client = client.clone();
    Arc::new(move |counter: Arc<AtomicU64>| {
        let (client, url) = (client.clone(), url.clone());
        async move {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| MeasureError::Http(e.to_string()))?;
            drain_counting(resp, &counter).await
        }
        .boxed()
    })
}

fn upload_factory(client: &reqwest::Client, url: String) -> StreamFactory {
    let client = client.clone();
    Arc::new(move |counter: Arc<AtomicU64>| {
        let (client, url) = (client.clone(), url.clone());
        async move {
            client
                .post(&url)
                .body(upload_body(UPLOAD_BYTES, counter))
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|e| MeasureError::Http(e.to_string()))?;
            Ok(())
        }
        .boxed()
    })
}

/// What a provider's endpoints look like; the rest of a run is shared.
pub(crate) struct Endpoints {
    pub latency_url: Box<dyn Fn() -> String + Send + Sync>,
    pub download_url: String,
    pub upload_url: String,
}

/// Latency, then download, then upload. Returns the stats and the download
/// and upload Mbps.
pub(crate) async fn measure_all(
    client: &reqwest::Client,
    opts: &RunOptions,
    ep: Endpoints,
) -> Result<(LatencyStats, f64, f64), ProviderError> {
    let stats = probe_latency(client, ep.latency_url).await?;
    let download =
        measure_throughput(&opts.throughput, download_factory(client, ep.download_url)).await?;
    let upload =
        measure_throughput(&opts.throughput, upload_factory(client, ep.upload_url)).await?;
    Ok((stats, download, upload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    fn headers(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append("server-timing", HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn server_timing_missing_header_is_none() {
        assert_eq!(server_timing_ms(&HeaderMap::new()), None);
    }

    #[test]
    fn server_timing_takes_first_dur_across_entries() {
        let h = headers(&["cfSpeedEdge;dur=8, cfSpeedWorker;dur=29"]);
        assert_eq!(server_timing_ms(&h), Some(8.0));
        let h = headers(&["cfRequestDuration;dur=12.5"]);
        assert_eq!(server_timing_ms(&h), Some(12.5));
    }

    #[test]
    fn server_timing_skips_entries_without_dur_and_spans_header_lines() {
        let h = headers(&["cfL4;desc=\"?proto=TCP\"", "cfSpeedWorker;desc=x;dur=29"]);
        assert_eq!(server_timing_ms(&h), Some(29.0));
        assert_eq!(server_timing_ms(&headers(&["miss, db;desc=x"])), None);
    }
}
