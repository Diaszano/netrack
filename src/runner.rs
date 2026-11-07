//! Single-flight test runner: lock, one retry, persistence, events and
//! automatic expected values.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use chrono::Utc;
use serde::Serialize;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::db::config::{self, AppConfig, ExpectedMode};
use crate::db::tests_repo::{self, Metrics, NewSpeedtest, TestKind};
use crate::measure::ThroughputConfig;
use crate::net;
use crate::providers::{Measurement, RunOptions, SpeedProvider, build_provider};

const EVENT_CAPACITY: usize = 64;
/// Successful tests needed (and used) to derive automatic expected values.
const EXPECTED_WINDOW: u32 = 10;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunEvent {
    Started {
        kind: TestKind,
    },
    Finished {
        id: i64,
        metrics: Metrics,
    },
    /// `id` is `None` (serialized as `null`) when no row could be stored.
    Failed {
        id: Option<i64>,
        error: String,
    },
    ExpectedUpdated {
        ping: f64,
        download: f64,
        upload: f64,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("a test is already running")]
    Busy,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

pub type ProviderFactory = Arc<dyn Fn(&AppConfig) -> Box<dyn SpeedProvider> + Send + Sync>;

#[derive(Clone)]
pub struct Runner {
    db: Db,
    provider_factory: ProviderFactory,
    busy: Arc<AtomicBool>,
    events: broadcast::Sender<RunEvent>,
}

/// Exclusive right to run a test, from [`Runner::try_acquire`]. Releases the
/// busy flag on drop, including during unwinding.
pub struct RunGuard(Arc<AtomicBool>);

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

impl Runner {
    pub fn new(db: Db, provider_factory: ProviderFactory) -> Runner {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Runner {
            db,
            provider_factory,
            busy: Arc::new(AtomicBool::new(false)),
            events,
        }
    }

    /// A runner that builds the provider selected in the config.
    pub fn with_default_providers(db: Db) -> Runner {
        Runner::new(db, Arc::new(|cfg| build_provider(cfg)))
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RunEvent> {
        self.events.subscribe()
    }

    pub fn is_running(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    fn emit(&self, event: RunEvent) {
        // No subscribers is fine.
        let _ = self.events.send(event);
    }

    /// Takes the single-flight lock without waiting; `Busy` if a test runs.
    pub fn try_acquire(&self) -> Result<RunGuard, RunError> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(RunError::Busy);
        }
        Ok(RunGuard(self.busy.clone()))
    }

    /// Runs one test (with a single retry on failure) and returns the id of
    /// the stored row. A failed test is stored too; only infrastructure
    /// problems surface as `Err`.
    pub async fn run(&self, kind: TestKind) -> Result<i64, RunError> {
        let guard = self.try_acquire()?;
        self.run_with_guard(guard, kind).await
    }

    /// [`Runner::run`] with the lock already held; the lock is released when
    /// the run ends. Every `Started` event is followed by `Finished` or `Failed`.
    pub async fn run_with_guard(&self, _guard: RunGuard, kind: TestKind) -> Result<i64, RunError> {
        let started = Instant::now();
        let cfg = config::load(&self.db).await?;
        self.emit(RunEvent::Started { kind });
        let result = self.run_loaded(&cfg, kind, started).await;
        if let Err(e) = &result {
            self.emit(RunEvent::Failed {
                id: None,
                error: e.to_string(),
            });
        }
        result
    }

    async fn run_loaded(
        &self,
        cfg: &AppConfig,
        kind: TestKind,
        started: Instant,
    ) -> Result<i64, RunError> {
        let provider: Arc<dyn SpeedProvider> = Arc::from((self.provider_factory)(cfg));
        let opts = RunOptions {
            local_addr: net::resolve_local_addr(cfg.interface.as_deref()),
            throughput: ThroughputConfig::default(),
        };

        let mut outcome = attempt(&provider, &opts).await;
        if let Err(e) = &outcome {
            tracing::warn!(error = %e, "speed test failed, retrying once");
            outcome = attempt(&provider, &opts).await;
        }
        let duration = started.elapsed().as_secs_f64();

        let (metrics, server) = match outcome {
            Ok(meas) => (Ok(metrics_of(&meas)), Some(meas)),
            Err(e) => (Err(e), None),
        };
        let new = NewSpeedtest {
            kind,
            provider: provider.name().to_owned(),
            server_id: server.as_ref().and_then(|s| s.server_id.clone()),
            server_name: server.as_ref().and_then(|s| s.server_name.clone()),
            server_host: server.as_ref().and_then(|s| s.server_host.clone()),
            result_id: server.as_ref().and_then(|s| s.result_id.clone()),
            metrics: metrics.clone(),
            duration,
        };
        let id = tests_repo::insert(&self.db, new, Utc::now()).await?;

        match metrics {
            Ok(metrics) => {
                self.emit(RunEvent::Finished { id, metrics });
                if let Err(e) = self.update_expected().await {
                    tracing::warn!(error = %e, "failed to update expected values");
                }
            }
            Err(error) => self.emit(RunEvent::Failed {
                id: Some(id),
                error,
            }),
        }
        Ok(id)
    }

    /// With `expected_mode=auto` and enough history, sets the expected values
    /// to the best of the last [`EXPECTED_WINDOW`] successful tests.
    async fn update_expected(&self) -> anyhow::Result<()> {
        let cfg = config::load(&self.db).await?;
        if cfg.expected_mode != ExpectedMode::Auto {
            return Ok(());
        }
        let rows = tests_repo::last_successful(&self.db, EXPECTED_WINDOW).await?;
        if rows.len() < EXPECTED_WINDOW as usize {
            return Ok(());
        }
        let ping = rows
            .iter()
            .filter_map(|r| r.ping)
            .fold(f64::INFINITY, f64::min);
        let download = rows.iter().filter_map(|r| r.download).fold(0.0, f64::max);
        let upload = rows.iter().filter_map(|r| r.upload).fold(0.0, f64::max);
        if !ping.is_finite() {
            return Ok(());
        }
        for (key, v) in [
            ("expected_ping", ping),
            ("expected_download", download),
            ("expected_upload", upload),
        ] {
            config::set(&self.db, key, &format!("{v:.2}")).await?;
        }
        let r2 = |v: f64| (v * 100.0).round() / 100.0;
        self.emit(RunEvent::ExpectedUpdated {
            ping: r2(ping),
            download: r2(download),
            upload: r2(upload),
        });
        Ok(())
    }
}

fn metrics_of(m: &Measurement) -> Metrics {
    Metrics {
        ping: m.ping,
        jitter: m.jitter,
        download: m.download,
        upload: m.upload,
    }
}

/// One provider attempt in its own task, so a panic becomes a failure.
async fn attempt(
    provider: &Arc<dyn SpeedProvider>,
    opts: &RunOptions,
) -> Result<Measurement, String> {
    let (provider, opts) = (provider.clone(), opts.clone());
    match tokio::spawn(async move { provider.run(&opts).await }).await {
        Ok(Ok(m)) => Ok(m),
        Ok(Err(e)) => Err(e.to_string()),
        Err(join) if join.is_panic() => Err("provider panicked".to_owned()),
        Err(join) => Err(format!("provider task failed: {join}")),
    }
}
