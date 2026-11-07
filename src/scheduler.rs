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
/// Longest single sleep, so wall-clock steps (NTP, fake-hwclock) are noticed.
const MAX_SLEEP: Duration = Duration::from_secs(30);
const RETRY_DELAY: Duration = Duration::from_secs(60);
/// How often [`wait_for_clock_past_history`] re-reads the clock.
const CLOCK_POLL: Duration = Duration::from_secs(30);
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

/// Source of wall-clock time; injectable so tests control it.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

/// Background cron scheduler plus the daily retention job. Dropping it stops both.
pub struct Scheduler {
    next_run: Arc<Mutex<Option<DateTime<Utc>>>>,
    tasks: Vec<JoinHandle<()>>,
}

impl Scheduler {
    /// `reload` must be signalled after any config change so the next fire
    /// time is recomputed from fresh config.
    pub fn spawn(db: Db, runner: Runner, reload: watch::Receiver<()>) -> Scheduler {
        Scheduler::spawn_with_clock(db, runner, reload, Arc::new(Utc::now))
    }

    pub fn spawn_with_clock(
        db: Db,
        runner: Runner,
        reload: watch::Receiver<()>,
        clock: Clock,
    ) -> Scheduler {
        let next_run = Arc::new(Mutex::new(None));
        let tasks = vec![
            tokio::spawn(schedule_loop(
                db.clone(),
                runner,
                reload,
                next_run.clone(),
                clock.clone(),
            )),
            tokio::spawn(retention_loop(db, clock)),
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

enum Wake {
    /// Wall time reached the target.
    Reached,
    /// Wall time moved backwards; the plan is stale.
    SteppedBack,
}

/// Waits until the wall clock reaches `target`, sleeping in bounded chunks and
/// re-reading the clock after each so clock steps in either direction are noticed.
async fn wait_until(clock: &Clock, target: DateTime<Utc>) -> Wake {
    let mut prev = clock();
    loop {
        let Ok(remaining) = (target - prev).to_std() else {
            return Wake::Reached;
        };
        if remaining.is_zero() {
            return Wake::Reached;
        }
        tokio::time::sleep(remaining.min(MAX_SLEEP)).await;
        let now = clock();
        if now < prev {
            return Wake::SteppedBack;
        }
        if now >= target {
            return Wake::Reached;
        }
        prev = now;
    }
}

async fn schedule_loop(
    db: Db,
    runner: Runner,
    mut reload: watch::Receiver<()>,
    shared: Arc<Mutex<Option<DateTime<Utc>>>>,
    clock: Clock,
) {
    wait_for_clock_past_history(&db, &clock).await;
    loop {
        let planned = plan(&db, clock()).await;
        let target = match &planned {
            Ok(t) => *t,
            Err(e) => {
                tracing::error!(error = %e, "cannot schedule next test; will retry");
                None
            }
        };
        *shared.lock().unwrap() = target;

        let Some(target) = target else {
            // Paused: only a config change matters. Error: retry on a timer too.
            let retry = async {
                if planned.is_err() {
                    tokio::time::sleep(RETRY_DELAY).await;
                } else {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                changed = reload.changed() => {
                    if changed.is_err() {
                        tracing::warn!("config reload channel closed; scheduler stopping");
                        return;
                    }
                }
                _ = retry => {}
            }
            continue;
        };
        tokio::select! {
            changed = reload.changed() => {
                if changed.is_err() {
                    tracing::warn!("config reload channel closed; scheduler stopping");
                    return;
                }
            }
            wake = wait_until(&clock, target) => {
                if matches!(wake, Wake::Reached) {
                    fire(&runner);
                }
            }
        }
    }
}

/// The next fire time strictly after `now` from fresh config, or `None` while paused.
async fn plan(db: &Db, now: DateTime<Utc>) -> anyhow::Result<Option<DateTime<Utc>>> {
    let cfg = config::load(db).await?;
    if cfg.paused {
        return Ok(None);
    }
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

async fn retention_loop(db: Db, clock: Clock) {
    loop {
        if let Err(e) = purge(&db, clock()).await {
            tracing::error!(error = %e, "retention job failed");
        }
        let now = clock().with_timezone(&db.tz());
        let next = match next_fire(RETENTION_CRON, false, now, &mut rand::rng()) {
            Ok(n) => n.with_timezone(&Utc),
            Err(e) => {
                tracing::error!(error = %e, "cannot schedule retention job");
                return;
            }
        };
        // A backward step replans; the purge on re-entry is harmless.
        wait_until(&clock, next).await;
    }
}

async fn purge(db: &Db, now: DateTime<Utc>) -> anyhow::Result<()> {
    let cfg = config::load(db).await.context("loading config")?;
    let removed = tests_repo::delete_older_than(db, cfg.retention_days, now).await?;
    if removed > 0 {
        tracing::info!(removed, "retention removed old speed tests");
    }
    crate::auth::delete_expired_sessions(db, now.timestamp()).await?;
    Ok(())
}

/// Waits until the wall clock is past the newest stored test. On a board
/// without RTC the clock can boot far behind until NTP syncs; tests run then
/// would be stored (and scheduled) at wrong times. Polls every
/// [`CLOCK_POLL`]; returns at once with no history or if it cannot be read.
pub async fn wait_for_clock_past_history(db: &Db, clock: &Clock) {
    let newest = match tests_repo::newest_created_at(db).await {
        Ok(Some(t)) => t,
        Ok(None) => return,
        Err(e) => {
            tracing::warn!(error = %e, "cannot read newest test time; not waiting for clock");
            return;
        }
    };
    let mut logged = false;
    while clock() < newest {
        if !logged {
            tracing::info!(
                now = %clock(),
                newest = %newest,
                "clock is behind the newest stored test; waiting for time sync"
            );
            logged = true;
        }
        tokio::time::sleep(CLOCK_POLL).await;
    }
}
