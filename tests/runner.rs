use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::db::Db;
use netrack::db::config;
use netrack::db::tests_repo::{self, TestKind};
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, RunError, RunEvent, Runner};

struct FakeProvider {
    script: Mutex<VecDeque<Result<Measurement, ProviderError>>>,
    calls: AtomicUsize,
    delay: Duration,
    panic: AtomicBool,
}

impl FakeProvider {
    fn new(script: Vec<Result<Measurement, ProviderError>>) -> Arc<FakeProvider> {
        Arc::new(FakeProvider {
            script: Mutex::new(script.into()),
            calls: AtomicUsize::new(0),
            delay: Duration::ZERO,
            panic: AtomicBool::new(false),
        })
    }
}

struct Shared(Arc<FakeProvider>);

impl SpeedProvider for Shared {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        let p = self.0.clone();
        async move {
            p.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(p.delay).await;
            if p.panic.load(Ordering::SeqCst) {
                panic!("boom");
            }
            p.script
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(m(10.0, 100.0, 50.0)))
        }
        .boxed()
    }
}

fn m(ping: f64, download: f64, upload: f64) -> Measurement {
    Measurement {
        ping,
        jitter: 1.0,
        download,
        upload,
        server_id: Some("7".into()),
        server_name: Some("Porto Alegre".into()),
        server_host: Some("poa.example.com".into()),
        result_id: Some("res-1".into()),
    }
}

fn err() -> Result<Measurement, ProviderError> {
    Err(ProviderError::Network("down".into()))
}

async fn setup(p: &Arc<FakeProvider>) -> (Db, Runner) {
    let db = Db::open_in_memory(chrono_tz::UTC).await.unwrap();
    let p = p.clone();
    let factory: ProviderFactory = Arc::new(move |_| Box::new(Shared(p.clone())));
    let runner = Runner::new(db.clone(), factory);
    (db, runner)
}

#[tokio::test]
async fn successful_run_persists_and_emits_finished() {
    let p = FakeProvider::new(vec![Ok(m(12.0, 300.0, 80.0))]);
    let (db, runner) = setup(&p).await;
    let mut rx = runner.subscribe();
    let id = runner.run(TestKind::Custom).await.unwrap();
    let row = tests_repo::get(&db, id).await.unwrap().unwrap();
    assert_eq!(row.provider, "fake");
    assert_eq!(row.kind, TestKind::Custom);
    assert_eq!(row.ping, Some(12.0));
    assert_eq!(row.download, Some(300.0));
    assert_eq!(row.server_name.as_deref(), Some("Porto Alegre"));
    assert_eq!(row.result_id.as_deref(), Some("res-1"));
    assert!(row.error.is_none());
    assert!(matches!(
        rx.recv().await.unwrap(),
        RunEvent::Started {
            kind: TestKind::Custom
        }
    ));
    match rx.recv().await.unwrap() {
        RunEvent::Finished { id: eid, metrics } => {
            assert_eq!(eid, id);
            assert_eq!(metrics.download, 300.0);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!runner.is_running());
}

#[tokio::test]
async fn slow_test_emits_degraded_right_after_finished() {
    let p = FakeProvider::new(vec![Ok(m(12.0, 10.0, 80.0))]);
    let (db, runner) = setup(&p).await;
    config::set(&db, "expected_mode", "manual").await.unwrap();
    config::set(&db, "expected_download", "100").await.unwrap();
    let mut rx = runner.subscribe();
    let id = runner.run(TestKind::Custom).await.unwrap();
    assert!(matches!(rx.recv().await.unwrap(), RunEvent::Started { .. }));
    assert!(matches!(
        rx.recv().await.unwrap(),
        RunEvent::Finished { .. }
    ));
    match rx.recv().await.unwrap() {
        RunEvent::Degraded {
            id: eid,
            expected_download,
            threshold_pct,
            ..
        } => {
            assert_eq!(eid, id);
            assert_eq!(threshold_pct, 70);
            assert_eq!(expected_download, 100.0);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn runner_records_failure_after_one_retry() {
    let p = FakeProvider::new(vec![err(), err()]);
    let (db, runner) = setup(&p).await;
    let mut rx = runner.subscribe();
    let id = runner.run(TestKind::Auto).await.unwrap();
    assert_eq!(p.calls.load(Ordering::SeqCst), 2);
    let rows = tests_repo::list(&db, None, 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].error.as_deref().unwrap().contains("down"));
    let _ = rx.recv().await.unwrap();
    assert!(
        matches!(rx.recv().await.unwrap(), RunEvent::Failed { id: eid, .. } if eid == Some(id))
    );
}

#[tokio::test]
async fn retry_success_stores_single_row() {
    let p = FakeProvider::new(vec![err(), Ok(m(9.0, 90.0, 40.0))]);
    let (db, runner) = setup(&p).await;
    runner.run(TestKind::Auto).await.unwrap();
    assert_eq!(p.calls.load(Ordering::SeqCst), 2);
    let rows = tests_repo::list(&db, None, 10).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].error.is_none());
    assert_eq!(rows[0].ping, Some(9.0));
}

#[tokio::test]
async fn concurrent_run_returns_busy() {
    let p = Arc::new(FakeProvider {
        script: Mutex::new(VecDeque::new()),
        calls: AtomicUsize::new(0),
        delay: Duration::from_millis(500),
        panic: AtomicBool::new(false),
    });
    let (_db, runner) = setup(&p).await;
    let r2 = runner.clone();
    let first = tokio::spawn(async move { r2.run(TestKind::Auto).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(runner.is_running());
    assert!(matches!(
        runner.run(TestKind::Custom).await,
        Err(RunError::Busy)
    ));
    first.await.unwrap().unwrap();
    assert!(!runner.is_running());
}

#[tokio::test]
async fn lock_released_when_provider_panics() {
    let p = FakeProvider::new(vec![]);
    p.panic.store(true, Ordering::SeqCst);
    let (db, runner) = setup(&p).await;
    let id = runner.run(TestKind::Auto).await.unwrap();
    let row = tests_repo::get(&db, id).await.unwrap().unwrap();
    assert!(row.error.is_some());
    assert!(!runner.is_running());
    p.panic.store(false, Ordering::SeqCst);
    let id2 = runner.run(TestKind::Auto).await.unwrap();
    let row2 = tests_repo::get(&db, id2).await.unwrap().unwrap();
    assert!(row2.error.is_none());
}

#[tokio::test]
async fn expected_values_auto_from_last_10() {
    let script = (0..10)
        .map(|i| Ok(m(20.0 - i as f64, 100.0 + i as f64 * 10.0, 50.0 + i as f64)))
        .collect();
    let p = FakeProvider::new(script);
    let (db, runner) = setup(&p).await;
    let mut rx = runner.subscribe();
    for _ in 0..10 {
        runner.run(TestKind::Auto).await.unwrap();
    }
    let cfg = config::load(&db).await.unwrap();
    assert_eq!(cfg.expected_ping, 11.0);
    assert_eq!(cfg.expected_download, 190.0);
    assert_eq!(cfg.expected_upload, 59.0);
    let mut saw = false;
    while let Ok(ev) = rx.try_recv() {
        if let RunEvent::ExpectedUpdated {
            ping,
            download,
            upload,
        } = ev
        {
            assert_eq!((ping, download, upload), (11.0, 190.0, 59.0));
            saw = true;
        }
    }
    assert!(saw);
}

#[tokio::test]
async fn expected_values_manual_unchanged() {
    let p = FakeProvider::new((0..10).map(|_| Ok(m(1.0, 999.0, 999.0))).collect());
    let (db, runner) = setup(&p).await;
    config::set(&db, "expected_mode", "manual").await.unwrap();
    let before = config::load(&db).await.unwrap();
    for _ in 0..10 {
        runner.run(TestKind::Auto).await.unwrap();
    }
    let after = config::load(&db).await.unwrap();
    assert_eq!(before.expected_ping, after.expected_ping);
    assert_eq!(before.expected_download, after.expected_download);
    assert_eq!(before.expected_upload, after.expected_upload);
}

#[tokio::test]
async fn insert_failure_after_started_emits_failed() {
    let p = FakeProvider::new(vec![]);
    let (db, runner) = setup(&p).await;
    db.call(|c| {
        c.execute_batch("DROP TABLE speedtests")?;
        Ok(())
    })
    .await
    .unwrap();
    let mut rx = runner.subscribe();
    assert!(matches!(
        runner.run(TestKind::Auto).await,
        Err(RunError::Internal(_))
    ));
    assert!(matches!(rx.try_recv().unwrap(), RunEvent::Started { .. }));
    match rx.try_recv().expect("a terminal event after Started") {
        RunEvent::Failed { id, error } => {
            assert_eq!(id, None);
            assert!(!error.is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(!runner.is_running());
}

#[tokio::test]
async fn config_load_failure_emits_no_started() {
    let p = FakeProvider::new(vec![]);
    let (db, runner) = setup(&p).await;
    db.call(|c| {
        c.execute(
            "INSERT INTO config (key, value) VALUES ('paused', 'garbage') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let mut rx = runner.subscribe();
    assert!(runner.run(TestKind::Auto).await.is_err());
    assert!(rx.try_recv().is_err(), "no event without a started run");
    assert_eq!(p.calls.load(Ordering::SeqCst), 0);
    assert!(!runner.is_running());
}

#[tokio::test]
async fn failed_event_without_row_serializes_null_id() {
    let ev = RunEvent::Failed {
        id: None,
        error: "db".into(),
    };
    assert_eq!(
        serde_json::to_value(&ev).unwrap(),
        serde_json::json!({ "type": "failed", "id": null, "error": "db" })
    );
}

#[tokio::test]
async fn try_acquire_is_exclusive_until_guard_dropped() {
    let p = FakeProvider::new(vec![]);
    let (_db, runner) = setup(&p).await;
    let guard = runner.try_acquire().unwrap();
    assert!(runner.is_running());
    assert!(matches!(runner.try_acquire(), Err(RunError::Busy)));
    assert!(matches!(
        runner.run(TestKind::Auto).await,
        Err(RunError::Busy)
    ));
    let id = runner
        .run_with_guard(guard, TestKind::Custom)
        .await
        .unwrap();
    assert!(id > 0);
    assert!(!runner.is_running());
}
