use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, anyhow};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use rand::{Rng, RngExt};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::db::Db;
use crate::db::config;
use crate::db::tests_repo::{self, TestKind};
use crate::runner::{RunError, Runner};

const MAX_OFFSET: Duration = Duration::from_secs(300);
const RETENTION_CRON: &str = "0 3 * * *";

/// Next cron occurrence strictly after `after`, in the timezone of `after`.
/// With `offset`, a uniform random delay in `0..min(5 min, interval / 4)` is
/// added, where `interval` is the gap to the following occurrence.
pub fn next_fire(
    cron: &str,
    offset: bool,
    after: DateTime<Tz>,
    rng: &mut impl Rng,
) -> anyhow::Result<DateTime<Tz>> {
    let cron: croner::Cron = cron
        .parse()
        .map_err(|e| anyhow!("invalid cron {cron:?}: {e}"))?;
    let next = cron.find_next_occurrence(&after, false)?;
    if !offset {
        return Ok(next);
    }
    let following = cron.find_next_occurrence(&next, false)?;
    let interval = (following - next).to_std().unwrap_or_default();
    let bound = (interval / 4).min(MAX_OFFSET);
    let bound_ms = u64::try_from(bound.as_millis()).unwrap_or(0);
    if bound_ms == 0 {
        return Ok(next);
    }
    let delay = rng.random_range(0..bound_ms);
    Ok(next + chrono::Duration::milliseconds(delay as i64))
}

/// Background cron scheduler plus the daily retention job. Dropping it stops both.
pub struct Scheduler {
    next_run: Arc<Mutex<Option<DateTime<Utc>>>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Scheduler {
    /// `reload` must be signalled after any config change so the next fire
    /// time is recomputed from fresh config.
    pub fn spawn(db: Db, runner: Runner, reload: watch::Receiver<()>) -> Scheduler {
        let next_run = Arc::new(Mutex::new(None));
        let tasks = vec![
            tokio::spawn(schedule_loop(db.clone(), runner, reload, next_run.clone())),
            tokio::spawn(retention_loop(db)),
        ];
        Scheduler { next_run, tasks }
    }

    /// When the next test will fire; `None` while paused (or if the cron is invalid).
    pub fn next_run(&self) -> Option<DateTime<Utc>> {
        *self.next_run.lock().unwrap()
    }
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        for t in &self.tasks {
            t.abort();
        }
    }
}

fn until(target: DateTime<Utc>) -> Duration {
    (target - Utc::now()).to_std().unwrap_or_default()
}

async fn schedule_loop(
    db: Db,
    runner: Runner,
    mut reload: watch::Receiver<()>,
    shared: Arc<Mutex<Option<DateTime<Utc>>>>,
) {
    // Never plan at or before a slot that already fired, even if the wall
    // clock lags the timer that woke us, so a slot cannot fire twice.
    let mut last_fired: Option<DateTime<Utc>> = None;
    loop {
        let target = match plan(&db, last_fired).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!(error = %e, "cannot schedule next test; waiting for config change");
                None
            }
        };
        *shared.lock().unwrap() = target;

        let Some(target) = target else {
            if reload.changed().await.is_err() {
                return;
            }
            continue;
        };
        tokio::select! {
            changed = reload.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            _ = tokio::time::sleep(until(target)) => {
                last_fired = Some(target);
                fire(&runner);
            }
        }
    }
}

/// The next fire time from fresh config, or `None` while paused.
async fn plan(db: &Db, floor: Option<DateTime<Utc>>) -> anyhow::Result<Option<DateTime<Utc>>> {
    let cfg = config::load(db).await?;
    if cfg.paused {
        return Ok(None);
    }
    let now = floor.map_or(Utc::now(), |f| f.max(Utc::now()));
    let now = now.with_timezone(&db.tz());
    let next = next_fire(&cfg.cron, cfg.schedule_offset, now, &mut rand::rng())?;
    Ok(Some(next.with_timezone(&Utc)))
}

/// Runs detached so that the loop being dropped or reloaded never cancels a
/// test mid-flight (which would release the runner lock while the provider
/// keeps going).
fn fire(runner: &Runner) {
    let runner = runner.clone();
    tokio::spawn(async move {
        match runner.run(TestKind::Auto).await {
            Ok(id) => tracing::info!(id, "scheduled test finished"),
            Err(RunError::Busy) => {
                tracing::info!("scheduled test skipped: another test is running")
            }
            Err(e) => tracing::error!(error = %e, "scheduled test failed"),
        }
    });
}

async fn retention_loop(db: Db) {
    loop {
        if let Err(e) = purge(&db).await {
            tracing::error!(error = %e, "retention job failed");
        }
        let now = Utc::now().with_timezone(&db.tz());
        let next = match next_fire(RETENTION_CRON, false, now, &mut rand::rng()) {
            Ok(n) => n.with_timezone(&Utc),
            Err(e) => {
                tracing::error!(error = %e, "cannot schedule retention job");
                return;
            }
        };
        tokio::time::sleep(until(next)).await;
    }
}

async fn purge(db: &Db) -> anyhow::Result<()> {
    let cfg = config::load(db).await.context("loading config")?;
    let removed = tests_repo::delete_older_than(db, cfg.retention_days, Utc::now()).await?;
    if removed > 0 {
        tracing::info!(removed, "retention removed old speed tests");
    }
    Ok(())
}
