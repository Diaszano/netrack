use chrono::NaiveDate;
use netrack::db::{Db, backup};

async fn open(dir: &std::path::Path) -> Db {
    Db::open(&dir.join("netrack.db"), chrono_tz::Tz::UTC)
        .await
        .unwrap()
}

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, d).unwrap()
}

#[tokio::test]
async fn creates_dated_backup() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(tmp.path()).await;
    let p = backup::run(&db, tmp.path(), day(7), 7)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(p, tmp.path().join("backups/netrack-2026-11-07.db"));
    let conn = rusqlite::Connection::open(&p).unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM speedtests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    // Same day again is a no-op returning the same path.
    assert_eq!(
        backup::run(&db, tmp.path(), day(7), 7).await.unwrap(),
        Some(p)
    );
}

#[tokio::test]
async fn keeps_only_n() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(tmp.path()).await;
    for d in 1..=9 {
        backup::run(&db, tmp.path(), day(d), 7).await.unwrap();
    }
    let mut names: Vec<_> = std::fs::read_dir(tmp.path().join("backups"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names.len(), 7);
    assert_eq!(names[0], "netrack-2026-11-03.db");
}

#[tokio::test]
async fn zero_disables() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(tmp.path()).await;
    assert_eq!(backup::run(&db, tmp.path(), day(7), 0).await.unwrap(), None);
    assert!(!tmp.path().join("backups").exists());
}
