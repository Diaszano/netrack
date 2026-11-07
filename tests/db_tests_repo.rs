use chrono::{DateTime, Duration, TimeZone, Utc};
use chrono_tz::Tz;
use netrack::db::{
    Db,
    tests_repo::{self, Metrics, NewSpeedtest, TestKind},
};

fn new_test(metrics: Result<Metrics, String>) -> NewSpeedtest {
    NewSpeedtest {
        kind: TestKind::Auto,
        provider: "cloudflare".into(),
        server_id: Some("1".into()),
        server_name: Some("Sao Paulo".into()),
        server_host: Some("gru.example.com".into()),
        result_id: Some("abc".into()),
        metrics,
        duration: 12.5,
    }
}

fn ok(ping: f64) -> Result<Metrics, String> {
    Ok(Metrics { ping, jitter: 1.0, download: 100.0, upload: 50.0 })
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 6, 12, 0, 0).unwrap()
}

#[tokio::test]
async fn insert_and_get_roundtrip() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let m = Metrics { ping: 12.345, jitter: 1.004, download: 587.123, upload: 40.999 };
    let id = tests_repo::insert(&db, new_test(Ok(m)), now()).await.unwrap();
    let row = tests_repo::get(&db, id).await.unwrap().unwrap();
    assert_eq!(row.id, id);
    assert_eq!(row.created_at, now().timestamp_millis());
    assert_eq!(row.kind, TestKind::Auto);
    assert_eq!(row.provider, "cloudflare");
    assert_eq!(row.server_name.as_deref(), Some("Sao Paulo"));
    assert_eq!(row.download, Some(587.12));
    assert_eq!(row.ping, Some(12.35));
    assert_eq!(row.jitter, Some(1.0));
    assert_eq!(row.upload, Some(41.0));
    assert_eq!(row.duration, Some(12.5));
    assert_eq!(row.error, None);
    assert!(tests_repo::get(&db, id + 1).await.unwrap().is_none());
}

#[tokio::test]
async fn failed_test_has_error_and_null_metrics() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let id = tests_repo::insert(&db, new_test(Err("timeout".into())), now()).await.unwrap();
    let row = tests_repo::get(&db, id).await.unwrap().unwrap();
    assert_eq!(row.error.as_deref(), Some("timeout"));
    assert_eq!((row.ping, row.jitter, row.download, row.upload), (None, None, None, None));
}

#[tokio::test]
async fn list_paginates_newest_first() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    for i in 0..15 {
        tests_repo::insert(&db, new_test(ok(i as f64)), now() + Duration::minutes(i)).await.unwrap();
    }
    let ids = |rows: Vec<tests_repo::SpeedtestRow>| rows.into_iter().map(|r| r.id).collect::<Vec<_>>();
    let page1 = tests_repo::list(&db, None, 10).await.unwrap();
    assert_eq!(ids(page1), (6..=15).rev().collect::<Vec<_>>());
    let page2 = tests_repo::list(&db, Some(6), 10).await.unwrap();
    assert_eq!(ids(page2), (1..=5).rev().collect::<Vec<_>>());
    assert_eq!(tests_repo::list(&db, None, 0).await.unwrap().len(), 1);
    assert_eq!(tests_repo::list(&db, None, 1000).await.unwrap().len(), 15);
}

#[tokio::test]
async fn delete_removes_row() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let id = tests_repo::insert(&db, new_test(ok(1.0)), now()).await.unwrap();
    assert!(tests_repo::delete(&db, id).await.unwrap());
    assert!(!tests_repo::delete(&db, id).await.unwrap());
    assert!(tests_repo::get(&db, id).await.unwrap().is_none());
}

#[tokio::test]
async fn delete_older_than_removes_only_old() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let old = tests_repo::insert(&db, new_test(ok(1.0)), now() - Duration::days(400)).await.unwrap();
    let recent = tests_repo::insert(&db, new_test(ok(2.0)), now() - Duration::days(10)).await.unwrap();
    assert_eq!(tests_repo::delete_older_than(&db, 365, now()).await.unwrap(), 1);
    assert!(tests_repo::get(&db, old).await.unwrap().is_none());
    assert!(tests_repo::get(&db, recent).await.unwrap().is_some());
}

#[tokio::test]
async fn last_successful_skips_failures() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let a = tests_repo::insert(&db, new_test(ok(1.0)), now()).await.unwrap();
    tests_repo::insert(&db, new_test(Err("boom".into())), now() + Duration::minutes(1)).await.unwrap();
    let c = tests_repo::insert(&db, new_test(ok(3.0)), now() + Duration::minutes(2)).await.unwrap();
    let rows = tests_repo::last_successful(&db, 5).await.unwrap();
    assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), vec![c, a]);
    assert_eq!(tests_repo::last_successful(&db, 1).await.unwrap().len(), 1);
}
