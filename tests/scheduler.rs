use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
use netrack::scheduler::{Scheduler, next_fire};
use rand::SeedableRng;
use rand::rngs::StdRng;
use tokio::sync::watch;

struct Counting(Arc<AtomicUsize>);

impl SpeedProvider for Counting {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        let calls = self.0.clone();
        async move {
            calls.fetch_add(1, Ordering::SeqCst);
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

async fn setup(calls: &Arc<AtomicUsize>) -> (Db, Runner) {
    let db = Db::open_in_memory(chrono_tz::UTC).await.unwrap();
    let calls = calls.clone();
    let factory: ProviderFactory = Arc::new(move |_| Box::new(Counting(calls.clone())));
    let runner = Runner::new(db.clone(), factory);
    (db, runner)
}

/// Advances virtual time in small steps, yielding so spawned tasks and
/// spawn_blocking database calls get to make progress.
async fn advance(total: Duration) {
    let step = Duration::from_secs(1);
    let mut elapsed = Duration::ZERO;
    while elapsed < total {
        tokio::time::advance(step).await;
        elapsed += step;
        for _ in 0..5 {
            tokio::task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    }
}

#[tokio::test(start_paused = true)]
async fn paused_scheduler_does_not_run() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (db, runner) = setup(&calls).await;
    config::set(&db, "cron", "* * * * *").await.unwrap();
    config::set(&db, "schedule_offset", "false").await.unwrap();
    config::set(&db, "paused", "true").await.unwrap();
    let (tx, rx) = watch::channel(());
    let sched = Scheduler::spawn(db.clone(), runner, rx);

    advance(Duration::from_secs(180)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sched.next_run().is_none());

    config::set(&db, "paused", "false").await.unwrap();
    tx.send(()).unwrap();
    advance(Duration::from_secs(61)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(sched.next_run().is_some());
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
    let _sched = Scheduler::spawn(db.clone(), runner, rx);

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
