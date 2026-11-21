use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::db::Db;
use netrack::db::config;
use netrack::db::tests_repo::{self, Metrics, NewSpeedtest, TestKind};
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::{Clock, Scheduler, next_fire, wait_for_clock_past_history};
use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::watch;

struct Counting {
    calls: Arc<AtomicUsize>,
    delay: Duration,
    fail: bool,
}

impl SpeedProvider for Counting {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        let calls = self.calls.clone();
        let (delay, fail) = (self.delay, self.fail);
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(delay).await;
            if fail {
                return Err(ProviderError::Network("down".into()));
            }
            Ok(Measurement {
                ping: 10.0,
                jitter: 1.0,
                download: 100.0,
                upload: 50.0,
                server_id: None,
                server_name: None,
                server_host: None,
                result_id: None,
            })
        }
        .boxed()
    }
}

fn utc(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

#[test]
fn next_fire_without_offset_is_exact() {
    let mut rng = StdRng::seed_from_u64(1);
    let after = utc("2026-03-10T10:15:00Z").with_timezone(&chrono_tz::UTC);
    let next = next_fire("0 * * * *", false, after, &mut rng).unwrap();
    assert_eq!(next.with_timezone(&Utc), utc("2026-03-10T11:00:00Z"));
}

#[test]
fn next_fire_offset_is_bounded() {
    let mut rng = StdRng::seed_from_u64(42);
    let after = utc("2026-03-10T10:15:00Z").with_timezone(&chrono_tz::UTC);
    let lo = utc("2026-03-10T11:00:00Z");
    let hi = utc("2026-03-10T11:05:00Z");
    let mut seen_nonzero = false;
    for _ in 0..1000 {
        let next = next_fire("0 * * * *", true, after, &mut rng)
            .unwrap()
            .with_timezone(&Utc);
        assert!(next >= lo && next < hi, "{next} out of range");
        seen_nonzero |= next > lo;
    }
    assert!(seen_nonzero);
}

#[test]
fn next_fire_respects_server_tz() {
    let mut rng = StdRng::seed_from_u64(1);
    let tz: Tz = chrono_tz::America::Sao_Paulo;
    let after = utc("2026-03-10T23:00:00Z").with_timezone(&tz);
    let next = next_fire("0 21 * * *", false, after, &mut rng).unwrap();
    assert_eq!(next.with_timezone(&Utc), utc("2026-03-11T00:00:00Z"));
}

#[test]
fn next_fire_rejects_invalid_cron() {
    let mut rng = StdRng::seed_from_u64(1);
    let after = Utc::now().with_timezone(&chrono_tz::UTC);
    assert!(next_fire("not a cron", false, after, &mut rng).is_err());
}

async fn setup_with(calls: &Arc<AtomicUsize>, delay: Duration, fail: bool) -> (Db, Runner) {
    let db = Db::open_in_memory(chrono_tz::UTC).await.unwrap();
    let calls = calls.clone();
    let factory: ProviderFactory = Arc::new(move |_| {
        Box::new(Counting {
            calls: calls.clone(),
            delay,
            fail,
        })
    });
    let runner = Runner::new(db.clone(), factory);
    (db, runner)
}

async fn setup(calls: &Arc<AtomicUsize>) -> (Db, Runner) {
    setup_with(calls, Duration::ZERO, false).await
}

/// Wall clock driven by tokio's (virtual) time plus manual steps.
struct TestClock {
    base: DateTime<Utc>,
    start: tokio::time::Instant,
    skew: Mutex<chrono::Duration>,
}

impl TestClock {
    fn new(base: &str) -> Arc<TestClock> {
        Arc::new(TestClock {
            base: utc(base),
            start: tokio::time::Instant::now(),
            skew: Mutex::new(chrono::Duration::zero()),
        })
    }
    fn now(&self) -> DateTime<Utc> {
        self.base
            + chrono::Duration::from_std(self.start.elapsed()).unwrap()
            + *self.skew.lock().unwrap()
    }
    fn step(&self, d: chrono::Duration) {
        *self.skew.lock().unwrap() += d;
    }
    fn clock(self: &Arc<Self>) -> Clock {
        let c = self.clone();
        Arc::new(move || c.now())
    }
}

/// Advances virtual time in 1s steps, yielding so spawned tasks and
/// spawn_blocking database calls get to make progress.
async fn advance(secs: u64) {
    for _ in 0..secs {
        tokio::time::advance(Duration::from_secs(1)).await;
        for _ in 0..5 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

/// Writes a config value bypassing validation.
async fn raw_set(db: &Db, key: &str, value: &str) {
    let (k, v) = (key.to_owned(), value.to_owned());
    db.call(move |c| {
        c.execute(
            "INSERT INTO config (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            (k, v),
        )?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn paused_scheduler_does_not_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    config::set(&db, "paused", "true").await.unwrap();
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    advance(180).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sched.next_run().is_none());

    config::set(&db, "paused", "false").await.unwrap();
    tx.send(()).unwrap();
    advance(61).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(sched.next_run().is_some());
}

#[tokio::test(start_paused = true)]
async fn failing_runs_do_not_stop_later_fires() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup_with(&calls, Duration::ZERO, true).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let _sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    advance(190).await;
    // Three fires, each attempt retried once by the runner.
    assert_eq!(calls.load(Ordering::SeqCst), 6);
}

#[tokio::test(start_paused = true)]
async fn busy_runner_does_not_stop_later_fires() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup_with(&calls, Duration::from_secs(150), false).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let _sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    // Fires at :00 (runs until +150s), +60 and +120 are Busy, +180... runs again.
    advance(250).await;
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn invalid_cron_recovers_after_reload() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "schedule_offset", "false").await.unwrap();
    raw_set(&db, "cron", "not a cron").await;
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    advance(120).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sched.next_run().is_none());

    config::set(&db, "cron", "* * * * *").await.unwrap();
    tx.send(()).unwrap();
    advance(61).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn backward_clock_jump_does_not_stall() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await; // default cron: hourly
    config::set(&db, "schedule_offset", "false").await.unwrap();
    let tc = TestClock::new("2026-03-10T10:30:00Z");
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());
    advance(5).await;
    assert_eq!(sched.next_run(), Some(utc("2026-03-10T11:00:00Z")));

    // NTP steps the clock back five hours: now 05:30, next slot 06:00.
    tc.step(chrono::Duration::hours(-5));
    advance(40).await;
    assert_eq!(sched.next_run(), Some(utc("2026-03-10T06:00:00Z")));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    advance(30 * 60).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn transient_load_error_recovers_via_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    raw_set(&db, "paused", "garbage").await; // makes config::load fail
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    advance(30).await;
    assert!(sched.next_run().is_none());
    raw_set(&db, "paused", "false").await; // healed, no reload signalled
    advance(100).await;
    assert!(calls.load(Ordering::SeqCst) >= 1);
}

fn ok_test() -> NewSpeedtest {
    NewSpeedtest {
        kind: TestKind::Auto,
        provider: "fake".into(),
        server_id: None,
        server_name: None,
        server_host: None,
        result_id: None,
        metrics: Ok(Metrics {
            ping: 1.0,
            jitter: 1.0,
            download: 1.0,
            upload: 1.0,
        }),
        duration: 1.0,
    }
}

#[tokio::test(start_paused = true)]
async fn waits_for_clock_to_pass_newest_row() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    tests_repo::insert(&db, ok_test(), utc("2026-03-10T11:00:00Z"))
        .await
        .unwrap();
    // Booted without RTC: the clock is an hour behind the newest row.
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let sched =
        Scheduler::spawn_with_clock(db.clone(), runner, rx, bk.path().to_path_buf(), tc.clock());

    advance(600).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sched.next_run().is_none());

    // NTP syncs: now 11:10:10, past the newest row.
    tc.step(chrono::Duration::hours(1));
    advance(100).await;
    assert!(calls.load(Ordering::SeqCst) >= 1);
    assert!(sched.next_run().is_some());
}

#[tokio::test(start_paused = true)]
async fn wait_for_clock_returns_at_once_without_history() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, _runner) = setup(&calls).await;
    let tc = TestClock::new("2026-03-10T10:00:10Z");
    tokio::time::timeout(
        Duration::from_millis(1),
        wait_for_clock_past_history(&db, &tc.clock()),
    )
    .await
    .expect("no rows: no wait");
}

#[tokio::test]
async fn retention_runs_at_startup() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "paused", "true").await.unwrap();
    let old = Utc::now() - chrono::Duration::days(400);
    let id = tests_repo::insert(
        &db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "fake".into(),
            server_id: None,
            server_name: None,
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 1.0,
                jitter: 1.0,
                download: 1.0,
                upload: 1.0,
            }),
            duration: 1.0,
        },
        old,
    )
    .await
    .unwrap();
    let (_tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let _sched = Scheduler::spawn(db.clone(), runner, rx, bk.path().to_path_buf());

    let mut gone = false;
    for _ in 0..100 {
        if tests_repo::get(&db, id).await.unwrap().is_none() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(gone, "old row should be deleted shortly after spawn");
}

#[tokio::test]
async fn backup_failure_does_not_stop_retention() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "paused", "true").await.unwrap();
    let id = tests_repo::insert(
        &db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "fake".into(),
            server_id: None,
            server_name: None,
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 1.0,
                jitter: 1.0,
                download: 1.0,
                upload: 1.0,
            }),
            duration: 1.0,
        },
        Utc::now() - chrono::Duration::days(400),
    )
    .await
    .unwrap();
    // A file where the backups dir should be makes create_dir_all fail.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("backups"), b"x").unwrap();
    let (_tx, rx) = watch::channel(());
    let _sched = Scheduler::spawn(db.clone(), runner, rx, dir.path().to_path_buf());

    let mut gone = false;
    for _ in 0..100 {
        if tests_repo::get(&db, id).await.unwrap().is_none() {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(gone, "retention must still run when the backup fails");
}
