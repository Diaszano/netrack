use chrono_tz::{America::Sao_Paulo, Tz};
use netrack::db::{
    Db,
    config::{self, ConfigError, Provider},
};

#[tokio::test]
async fn defaults_loaded_on_fresh_db() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let c = config::load(&db).await.unwrap();
    assert_eq!(c.provider, Provider::Cloudflare);
    assert_eq!(c.cron, "0 * * * *");
    assert_eq!(c.retention_days, 365);
    assert_eq!((c.expected_ping, c.expected_download, c.expected_upload), (25.0, 100.0, 50.0));
    assert!(!c.paused);
    assert!(c.schedule_offset);
    assert_eq!((c.libre_server_id, c.libre_url, c.interface), (None, None, None));
}

#[tokio::test]
async fn set_persists_value() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    config::set(&db, "retention_days", "30").await.unwrap();
    assert_eq!(config::load(&db).await.unwrap().retention_days, 30);
    config::set(&db, "libre_server_id", "12").await.unwrap();
    config::set(&db, "provider", "librespeed").await.unwrap();
    let c = config::load(&db).await.unwrap();
    assert_eq!((c.libre_server_id, c.provider), (Some(12), Provider::LibreSpeed));
}

#[tokio::test]
async fn config_rejects_invalid_values() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    let before = config::load(&db).await.unwrap();
    for (k, v) in [
        ("cron", "nope"),
        ("retention_days", "0"),
        ("retention_days", "10001"),
        ("provider", "foo"),
        ("expected_download", "-1"),
        ("libre_url", "ftp://x"),
    ] {
        let err = config::set(&db, k, v).await.unwrap_err();
        assert!(matches!(err, ConfigError::Invalid { .. }), "{k}={v}: {err:?}");
    }
    assert_eq!(config::load(&db).await.unwrap(), before);
    let err = config::set(&db, "nope", "1").await.unwrap_err();
    assert!(matches!(err, ConfigError::UnknownKey(_)));
}

#[tokio::test]
async fn local_fields_recomputed_when_tz_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("netrack.db");
    // 2026-01-15T02:30:00Z
    let ts: i64 = 1_768_444_200_000;
    let db = Db::open(&path, Tz::UTC).await.unwrap();
    db.call(move |c| {
        c.execute(
            "INSERT INTO speedtests (created_at, local_hour, local_weekday, kind, provider) \
             VALUES (?1, 2, 3, 'auto', 'cloudflare')",
            [ts],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    drop(db);

    let db = Db::open(&path, Sao_Paulo).await.unwrap();
    let (h, w): (i64, i64) = db
        .call(|c| Ok(c.query_row("SELECT local_hour, local_weekday FROM speedtests", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
        .await
        .unwrap();
    assert_eq!((h, w), (23, 2));
}

#[tokio::test]
async fn valid_cron_persists_and_survives_rejected_set() {
    let db = Db::open_in_memory(Tz::UTC).await.unwrap();
    config::set(&db, "cron", "*/15 * * * *").await.unwrap();
    assert_eq!(config::load(&db).await.unwrap().cron, "*/15 * * * *");
    config::set(&db, "retention_days", "30").await.unwrap();
    assert!(config::set(&db, "retention_days", "0").await.is_err());
    assert!(config::set(&db, "cron", "nope").await.is_err());
    let c = config::load(&db).await.unwrap();
    assert_eq!((c.retention_days, c.cron.as_str()), (30, "*/15 * * * *"));
}
