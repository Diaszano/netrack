use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Tz;
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::auth::create_session;
use netrack::db::Db;
use netrack::db::config::{self, PasswordMode};
use netrack::db::tests_repo::{Metrics, NewSpeedtest, TestKind, insert};
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

struct DummyProvider;

impl SpeedProvider for DummyProvider {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move {
            Ok(Measurement {
                ping: 10.0,
                jitter: 1.0,
                download: 100.0,
                upload: 50.0,
                server_id: Some("1".into()),
                server_name: Some("Test".into()),
                server_host: Some("localhost".into()),
                result_id: None,
            })
        }
        .boxed()
    }
}

struct TestApp {
    app: Router,
    db: Db,
    _tz: Tz,
}

impl TestApp {
    async fn new() -> Self {
        Self::with_tz(chrono_tz::Tz::UTC).await
    }

    async fn with_tz(tz: Tz) -> Self {
        let db = Db::open_in_memory(tz).await.unwrap();
        let factory: ProviderFactory = Arc::new(|_| Box::new(DummyProvider));
        let runner = Runner::new(db.clone(), factory);
        let (tx, rx) = watch::channel(());
        let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx);
        let state = AppState::new(db.clone(), runner, tx, Arc::new(scheduler));
        let app = router(state);
        Self { app, db, _tz: tz }
    }

    async fn get(
        &self,
        uri: &str,
        cookie: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut req = Request::builder().method(Method::GET).uri(uri);
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }

        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&bytes).into_owned();
        (status, headers, body)
    }

    async fn post(
        &self,
        uri: &str,
        body: impl Into<String>,
        content_type: Option<&str>,
        cookie: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut req = Request::builder().method(Method::POST).uri(uri);
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }

        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body.into())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8_lossy(&bytes).into_owned();
        (status, headers, body)
    }

    async fn set_password(&self, password: &str, mode: PasswordMode) {
        let payload = json!({
            "password": password,
            "mode": mode.as_str(),
        });
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/auth/password")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }
}

#[allow(clippy::too_many_arguments)]
async fn insert_sample_test(
    app: &TestApp,
    server_name: Option<&str>,
    download: f64,
    upload: f64,
    ping: f64,
    jitter: f64,
    duration: f64,
    at: DateTime<Utc>,
) -> i64 {
    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "cloudflare".into(),
            server_id: Some("1".into()),
            server_name: server_name.map(Into::into),
            server_host: Some("gru.cloudflare.com".into()),
            result_id: Some("res-1".into()),
            metrics: Ok(Metrics {
                ping,
                jitter,
                download,
                upload,
            }),
            duration,
        },
        at,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn test_export_json_empty_database() {
    let app = TestApp::new().await;
    let (status, headers, body) = app.get("/api/data/export?format=json", None).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("application/json"),
        "expected json content type, got: {content_type}"
    );

    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        disposition.starts_with("attachment; filename=\"netrack-export-"),
        "unexpected disposition: {disposition}"
    );
    assert!(disposition.ends_with(".json\""));

    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed, json!([]));
}

#[tokio::test]
async fn test_export_csv_with_data() {
    let app = TestApp::new().await;
    let test_time = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
    insert_sample_test(
        &app,
        Some("São Paulo, SP"),
        150.25,
        50.12,
        12.34,
        1.55,
        15.2,
        test_time,
    )
    .await;

    let (status, headers, body) = app.get("/api/data/export?format=csv", None).await;
    assert_eq!(status, StatusCode::OK);

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/csv"),
        "expected csv content type, got: {content_type}"
    );

    let disposition = headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        disposition.starts_with("attachment; filename=\"netrack-export-"),
        "unexpected disposition: {disposition}"
    );
    assert!(disposition.ends_with(".csv\""));

    let lines: Vec<&str> = body.trim().lines().collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0],
        "id,created_at,kind,provider,server_id,server_name,server_host,result_id,ping,jitter,download,upload,duration,error"
    );
    assert!(
        lines[1].contains("\"São Paulo, SP\""),
        "escaped server name not found: {}",
        lines[1]
    );
    assert!(lines[1].contains("150.25"));
    assert!(lines[1].contains("50.12"));
}

#[tokio::test]
async fn test_export_and_import_native_roundtrip() {
    let app1 = TestApp::new().await;
    let time1 = Utc.with_ymd_and_hms(2026, 11, 7, 10, 0, 0).unwrap();
    let time2 = Utc.with_ymd_and_hms(2026, 11, 7, 11, 0, 0).unwrap();
    insert_sample_test(
        &app1,
        Some("Server 1"),
        200.0,
        100.0,
        15.0,
        2.0,
        14.0,
        time1,
    )
    .await;
    insert_sample_test(&app1, Some("Server 2"), 350.0, 150.0, 8.0, 0.8, 12.5, time2).await;

    // Export from app1
    let (status, _, export_body) = app1.get("/api/data/export?format=json", None).await;
    assert_eq!(status, StatusCode::OK);

    // Import into fresh app2
    let app2 = TestApp::new().await;
    let (import_status, _, import_resp) = app2
        .post(
            "/api/data/import",
            export_body,
            Some("application/json"),
            None,
        )
        .await;
    assert_eq!(import_status, StatusCode::OK);
    let import_json: Value = serde_json::from_str(&import_resp).unwrap();
    assert_eq!(import_json, json!({ "imported": 2, "skipped": 0 }));

    // Verify stats in app2
    let (stats_status, _, stats_resp) = app2.get("/api/data/stats", None).await;
    assert_eq!(stats_status, StatusCode::OK);
    let stats_json: Value = serde_json::from_str(&stats_resp).unwrap();
    assert_eq!(stats_json["rowCount"], 2);
}

#[tokio::test]
async fn test_import_myspeed_v1_format() {
    // App with Sao Paulo timezone (-03:00)
    let sp_tz: Tz = "America/Sao_Paulo".parse().unwrap();
    let app = TestApp::with_tz(sp_tz).await;

    let myspeed_payload = json!([
        {
            "ping": 15.4,
            "jitter": 1.2,
            "download": 120.5,
            "upload": 45.2,
            "server": "Frankfurt - Speedtest",
            "createdOn": "2026-11-07T15:00:00.000Z",
            "time": 14000
        },
        {
            "ping": 20.0,
            "jitter": 3.5,
            "download": 250.0,
            "upload": 80.0,
            "type": "ookla",
            "server": {
                "id": "42",
                "name": "São Paulo Ookla",
                "host": "sp.ookla.com"
            },
            "createdOn": "2026-11-07T18:00:00Z"
        },
        {
            "ping": null,
            "jitter": null,
            "download": null,
            "upload": null,
            "error": "Connection timed out",
            "createdOn": "2026-11-07T19:00:00Z"
        }
    ]);

    let (status, _, body) = app
        .post(
            "/api/data/import",
            myspeed_payload.to_string(),
            Some("application/json"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp, json!({ "imported": 3, "skipped": 0 }));

    // Check stats
    let (stats_status, _, stats_body) = app.get("/api/data/stats", None).await;
    assert_eq!(stats_status, StatusCode::OK);
    let stats: Value = serde_json::from_str(&stats_body).unwrap();
    assert_eq!(stats["rowCount"], 3);

    // Verify local_hour calculation in database for America/Sao_Paulo (UTC-3)
    // 15:00:00 UTC = 12:00:00 local time
    let local_hour: u32 = app
        .db
        .call(|conn| {
            conn.query_row(
                "SELECT local_hour FROM speedtests WHERE server_name = 'Frankfurt - Speedtest'",
                [],
                |r| r.get(0),
            )
            .map_err(Into::into)
        })
        .await
        .unwrap();
    assert_eq!(local_hour, 12);
}

#[tokio::test]
async fn test_import_csv_format() {
    let app = TestApp::new().await;
    let now = Utc::now().timestamp_millis();
    let csv_content = format!(
        "id,created_at,kind,provider,server_id,server_name,server_host,result_id,ping,jitter,download,upload,duration,error\n\
1,{},auto,cloudflare,1,São Paulo,gru.cloudflare.com,res-1,12.34,1.23,150.45,45.67,15.2,\n\
2,{},custom,librespeed,2,Rio de Janeiro,gig.librespeed.org,res-2,18.50,2.10,210.00,85.00,12.0,\n",
        now - 3_600_000,
        now,
    );

    let (status, _, body) = app
        .post("/api/data/import", csv_content, Some("text/csv"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp, json!({ "imported": 2, "skipped": 0 }));

    let (stats_status, _, stats_body) = app.get("/api/data/stats", None).await;
    assert_eq!(stats_status, StatusCode::OK);
    let stats: Value = serde_json::from_str(&stats_body).unwrap();
    assert_eq!(stats["rowCount"], 2);
}

#[tokio::test]
async fn test_import_invalid_data_rolls_back() {
    let app = TestApp::new().await;
    // Payload where the second record has an invalid timestamp
    let invalid_payload = json!([
        {
            "ping": 10.0,
            "download": 100.0,
            "upload": 50.0,
            "createdOn": "2026-11-07T12:00:00Z"
        },
        {
            "ping": 10.0,
            "download": 100.0,
            "upload": 50.0,
            "createdOn": "not-a-valid-date"
        }
    ]);

    let (status, _, _) = app
        .post(
            "/api/data/import",
            invalid_payload.to_string(),
            Some("application/json"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Ensure 0 rows inserted (rollback)
    let (_, _, stats_body) = app.get("/api/data/stats", None).await;
    let stats: Value = serde_json::from_str(&stats_body).unwrap();
    assert_eq!(stats["rowCount"], 0);
}

#[tokio::test]
async fn test_data_stats_size_and_count() {
    let app = TestApp::new().await;
    let (status, _, body) = app.get("/api/data/stats", None).await;
    assert_eq!(status, StatusCode::OK);
    let stats: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(stats["rowCount"], 0);
    assert!(
        stats["dbSizeBytes"].as_u64().unwrap() > 0,
        "db size must be greater than 0"
    );

    insert_sample_test(&app, Some("Test"), 100.0, 50.0, 10.0, 1.0, 10.0, Utc::now()).await;

    let (_, _, body2) = app.get("/api/data/stats", None).await;
    let stats2: Value = serde_json::from_str(&body2).unwrap();
    assert_eq!(stats2["rowCount"], 1);
}

#[tokio::test]
async fn test_reset_clears_history_and_restores_config() {
    let app = TestApp::new().await;

    // 1. Insert speedtest
    insert_sample_test(
        &app,
        Some("To be wiped"),
        100.0,
        50.0,
        10.0,
        1.0,
        10.0,
        Utc::now(),
    )
    .await;

    // 2. Modify config
    config::set(&app.db, "cron", "*/5 * * * *").await.unwrap();

    // 3. Create a session in sessions table
    let (token, _) = create_session(&app.db).await.unwrap();

    // 4. Reset
    let (status, _, body) = app
        .post(
            "/api/data/reset",
            "{}",
            Some("application/json"),
            Some(&format!("netrack_session={token}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(resp, json!({ "reset": true }));

    // 5. Verify speedtests table is wiped
    let (_, _, stats_body) = app.get("/api/data/stats", None).await;
    let stats: Value = serde_json::from_str(&stats_body).unwrap();
    assert_eq!(stats["rowCount"], 0);

    // 6. Verify config restored to defaults
    let cfg = config::load(&app.db).await.unwrap();
    assert_eq!(cfg.cron, "0 * * * *");
    assert_eq!(cfg.password_mode, PasswordMode::None);

    // 7. Verify session remains intact
    let valid = netrack::auth::validate_session(&app.db, &token)
        .await
        .unwrap();
    assert!(valid, "session should remain intact after reset");
}

#[tokio::test]
async fn test_reset_auth_protection() {
    let app = TestApp::new().await;
    app.set_password("secret123", PasswordMode::Public).await;

    // Public mode: unauthenticated POST /api/data/reset -> 401
    let (status, _, _) = app
        .post("/api/data/reset", "{}", Some("application/json"), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Create session
    let (token, _) = create_session(&app.db).await.unwrap();
    let (status, _, _) = app
        .post(
            "/api/data/reset",
            "{}",
            Some("application/json"),
            Some(&format!("netrack_session={token}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Set to protected mode
    app.set_password("secret123", PasswordMode::Protected).await;
    let (status, _, _) = app
        .post("/api/data/reset", "{}", Some("application/json"), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (token2, _) = create_session(&app.db).await.unwrap();
    let (status, _, _) = app
        .post(
            "/api/data/reset",
            "{}",
            Some("application/json"),
            Some(&format!("netrack_session={token2}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_import_larger_than_default_body_limit() {
    let app = TestApp::new().await;
    let records: Vec<Value> = (0..10_000i64)
        .map(|i| {
            json!({
                "createdAt": 1_700_000_000_000i64 + i * 3_600_000,
                "kind": "auto",
                "provider": "librespeed",
                "download": 100.0,
                "upload": 50.0,
                "ping": 10.0,
                "jitter": 1.0,
                "serverName": "x".repeat(200),
            })
        })
        .collect();
    let body = serde_json::to_string(&records).unwrap();
    assert!(body.len() > 2 * 1024 * 1024);

    let (status, _, resp) = app
        .post("/api/data/import", body, Some("application/json"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(resp, json!({ "imported": 10000, "skipped": 0 }));
}

#[tokio::test]
async fn test_import_twice_does_not_duplicate() {
    let app = TestApp::new().await;
    let base = Utc.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
    for i in 0..3 {
        insert_sample_test(
            &app,
            Some("S"),
            100.0,
            50.0,
            10.0,
            1.0,
            10.0,
            base + chrono::Duration::hours(i),
        )
        .await;
    }
    let (_, _, export_body) = app.get("/api/data/export?format=json", None).await;
    let (status, _, resp) = app
        .post(
            "/api/data/import",
            export_body,
            Some("application/json"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(resp, json!({ "imported": 0, "skipped": 3 }));

    let (_, _, stats) = app.get("/api/data/stats", None).await;
    let stats: Value = serde_json::from_str(&stats).unwrap();
    assert_eq!(stats["rowCount"], 3);
}

#[tokio::test]
async fn test_import_duplicates_inside_same_file() {
    let app = TestApp::new().await;
    let row = "1,1700000000000,auto,cloudflare,1,SP,h,r,12.0,1.0,150.0,45.0,15.0,\n";
    let csv = format!(
        "id,created_at,kind,provider,server_id,server_name,server_host,result_id,ping,jitter,download,upload,duration,error\n{row}{row}"
    );
    let (status, _, resp) = app
        .post("/api/data/import", csv, Some("text/csv"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let resp: Value = serde_json::from_str(&resp).unwrap();
    assert_eq!(resp, json!({ "imported": 1, "skipped": 1 }));
}
