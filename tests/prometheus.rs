use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::Utc;
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::auth::create_session;
use netrack::db::Db;
use netrack::db::config::PasswordMode;
use netrack::db::tests_repo::{Metrics, NewSpeedtest, TestKind, insert};
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
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

fn encode_base64(input: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i];
        let b1 = if i + 1 < input.len() { input[i + 1] } else { 0 };
        let b2 = if i + 2 < input.len() { input[i + 2] } else { 0 };
        out.push(CHARS[(b0 >> 2) as usize] as char);
        out.push(CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < input.len() {
            out.push(CHARS[(((b1 & 0x0F) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(CHARS[(b2 & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

struct TestApp {
    app: Router,
    db: Db,
    _bk: tempfile::TempDir,
}

impl TestApp {
    async fn new() -> Self {
        let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
        let factory: ProviderFactory = Arc::new(|_| Box::new(DummyProvider));
        let runner = Runner::new(db.clone(), factory);
        let (tx, rx) = watch::channel(());
        let bk = tempfile::tempdir().unwrap();
        let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx, bk.path().to_path_buf());
        let state = AppState::new(db.clone(), runner, tx, Arc::new(scheduler));
        let app = router(state);
        Self { app, db, _bk: bk }
    }

    async fn get(
        &self,
        uri: &str,
        cookie: Option<&str>,
        auth_header: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, String) {
        let mut req = Request::builder().method(Method::GET).uri(uri);
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        if let Some(a) = auth_header {
            req = req.header("authorization", a);
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

    async fn set_password(&self, password: &str, mode: PasswordMode) {
        let payload = serde_json::json!({
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

#[tokio::test]
async fn prometheus_metrics_empty_database() {
    let app = TestApp::new().await;
    let (status, headers, body) = app.get("/api/prometheus/metrics", None, None).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/plain"),
        "expected text/plain content type, got: {content_type}"
    );

    // Should include HELP and TYPE for all 7 metrics
    assert!(body.contains("# HELP netrack_ping_latency_ms"));
    assert!(body.contains("# TYPE netrack_ping_latency_ms gauge"));
    assert!(body.contains("# HELP netrack_jitter_ms"));
    assert!(body.contains("# TYPE netrack_jitter_ms gauge"));
    assert!(body.contains("# HELP netrack_download_speed_mbps"));
    assert!(body.contains("# TYPE netrack_download_speed_mbps gauge"));
    assert!(body.contains("# HELP netrack_upload_speed_mbps"));
    assert!(body.contains("# TYPE netrack_upload_speed_mbps gauge"));
    assert!(body.contains("# HELP netrack_test_duration_seconds"));
    assert!(body.contains("# TYPE netrack_test_duration_seconds gauge"));
    assert!(body.contains("# HELP netrack_last_test_timestamp_seconds"));
    assert!(body.contains("# TYPE netrack_last_test_timestamp_seconds gauge"));
    assert!(body.contains("# HELP netrack_test_success"));
    assert!(body.contains("# TYPE netrack_test_success gauge"));
}

#[tokio::test]
async fn prometheus_metrics_with_successful_test() {
    let app = TestApp::new().await;
    let at = Utc::now();
    let at_secs = at.timestamp();

    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "librespeed".into(),
            server_id: Some("101".into()),
            server_name: Some("Local Server".into()),
            server_host: Some("localhost".into()),
            result_id: None,
            metrics: Ok(Metrics {
                ping: 12.5,
                jitter: 1.5,
                download: 120.5,
                upload: 45.0,
            }),
            duration: 10.2,
        },
        at,
    )
    .await
    .unwrap();

    let (status, headers, body) = app.get("/api/prometheus/metrics", None, None).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(content_type.contains("text/plain"));

    assert!(body.contains("netrack_ping_latency_ms{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 12.5"));
    assert!(body.contains("netrack_jitter_ms{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 1.5"));
    assert!(body.contains("netrack_download_speed_mbps{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 120.5"));
    assert!(
        body.contains("netrack_upload_speed_mbps{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 45")
    );
    assert!(body.contains("netrack_test_duration_seconds{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 10.2"));
    assert!(body.contains(&format!("netrack_last_test_timestamp_seconds{{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"}} {at_secs}")));
    assert!(body.contains("netrack_test_success{provider=\"librespeed\",server_id=\"101\",server_name=\"Local Server\"} 1"));
}

#[tokio::test]
async fn prometheus_metrics_returns_latest_test() {
    let app = TestApp::new().await;
    let now = Utc::now();
    let t1 = now - chrono::Duration::hours(2);
    let t2 = now - chrono::Duration::hours(1);
    let t2_secs = t2.timestamp();

    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "dummy".into(),
            server_id: Some("1".into()),
            server_name: Some("Old".into()),
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 20.0,
                jitter: 3.0,
                download: 50.0,
                upload: 25.0,
            }),
            duration: 8.0,
        },
        t1,
    )
    .await
    .unwrap();

    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "cloudflare".into(),
            server_id: Some("GRU".into()),
            server_name: Some("Sao Paulo".into()),
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 8.0,
                jitter: 0.5,
                download: 300.0,
                upload: 150.0,
            }),
            duration: 12.0,
        },
        t2,
    )
    .await
    .unwrap();

    let (status, _, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body.contains("netrack_download_speed_mbps{provider=\"cloudflare\",server_id=\"GRU\",server_name=\"Sao Paulo\"} 300"));
    assert!(!body.contains("provider=\"dummy\""));
    assert!(body.contains(&format!("netrack_last_test_timestamp_seconds{{provider=\"cloudflare\",server_id=\"GRU\",server_name=\"Sao Paulo\"}} {t2_secs}")));
}

#[tokio::test]
async fn prometheus_metrics_with_failed_test() {
    let app = TestApp::new().await;
    let at = Utc::now();

    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "ookla".into(),
            server_id: Some("999".into()),
            server_name: Some("Ookla Server".into()),
            server_host: None,
            result_id: None,
            metrics: Err("connection timeout".into()),
            duration: 5.0,
        },
        at,
    )
    .await
    .unwrap();

    let (status, _, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body.contains(
        "netrack_test_success{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 0"
    ));
    assert!(body.contains("netrack_ping_latency_ms{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 0"));
    assert!(body.contains("netrack_download_speed_mbps{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 0"));
    assert!(body.contains("netrack_upload_speed_mbps{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 0"));
    assert!(body.contains(
        "netrack_jitter_ms{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 0"
    ));
    assert!(body.contains("netrack_test_duration_seconds{provider=\"ookla\",server_id=\"999\",server_name=\"Ookla Server\"} 5"));
}

#[tokio::test]
async fn prometheus_metrics_labels_null_and_escaping() {
    let app = TestApp::new().await;
    let now = Utc::now();
    let at = now - chrono::Duration::hours(1);

    // Test with None for server_id and server_name
    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "cloudflare".into(),
            server_id: None,
            server_name: None,
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 15.0,
                jitter: 2.0,
                download: 200.0,
                upload: 100.0,
            }),
            duration: 10.0,
        },
        at,
    )
    .await
    .unwrap();

    let (status, _, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK);

    // Must handle null cleanly without "None" or panicking
    assert!(body.contains(
        "netrack_download_speed_mbps{provider=\"cloudflare\",server_id=\"\",server_name=\"\"} 200"
    ));
    assert!(!body.contains("None"));
    assert!(!body.contains("null"));

    // Test with special characters in server_name
    let at2 = now;
    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Custom,
            provider: "custom".into(),
            server_id: Some("id\"1\"".into()),
            server_name: Some("Server \"Fast\"\nSpecial\\Path".into()),
            server_host: None,
            result_id: None,
            metrics: Ok(Metrics {
                ping: 5.0,
                jitter: 0.5,
                download: 500.0,
                upload: 250.0,
            }),
            duration: 6.0,
        },
        at2,
    )
    .await
    .unwrap();

    let (status, _, body2) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK);
    // Quotes and backslashes escaped
    assert!(body2.contains("server_id=\"id\\\"1\\\"\""));
    assert!(body2.contains("server_name=\"Server \\\"Fast\\\"\\nSpecial\\\\Path\""));
}

#[tokio::test]
async fn prometheus_auth_mode_none() {
    let app = TestApp::new().await;
    let (status, _, _) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn prometheus_auth_mode_public() {
    let app = TestApp::new().await;
    app.set_password("mypassword", PasswordMode::Public).await;

    // 1. Unauthenticated request -> 401 with WWW-Authenticate header
    let (status, headers, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers
            .get(header::WWW_AUTHENTICATE)
            .and_then(|h| h.to_str().ok()),
        Some("Basic realm=\"netrack\"")
    );
    assert!(body.contains("unauthorized"));

    // 2. Invalid Basic Auth password -> 401
    let bad_cred = encode_base64(b"netrack:wrongpass");
    let (status, headers, _) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {bad_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers
            .get(header::WWW_AUTHENTICATE)
            .and_then(|h| h.to_str().ok()),
        Some("Basic realm=\"netrack\"")
    );

    // 3. Valid Basic Auth with "netrack" username -> 200 OK
    let good_cred = encode_base64(b"netrack:mypassword");
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {good_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));

    // 4. Valid Basic Auth with any username (e.g. "prom_scraper") -> 200 OK
    let any_user_cred = encode_base64(b"prom_scraper:mypassword");
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {any_user_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));

    // 5. Valid session cookie -> 200 OK
    let (token, _) = create_session(&app.db).await.unwrap();
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            Some(&format!("netrack_session={token}")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));

    // 6. Valid Bearer token -> 200 OK
    let (token2, _) = create_session(&app.db).await.unwrap();
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Bearer {token2}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));

    // 7. Malformed Basic Auth -> 401
    let (status, _, _) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some("Basic not_valid_base64!"),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let no_colon_cred = encode_base64(b"netrack");
    let (status, _, _) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {no_colon_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn prometheus_auth_mode_protected() {
    let app = TestApp::new().await;
    app.set_password("mypassword", PasswordMode::Protected)
        .await;

    // 1. Unauthenticated request -> 401 with WWW-Authenticate header
    let (status, headers, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers
            .get(header::WWW_AUTHENTICATE)
            .and_then(|h| h.to_str().ok()),
        Some("Basic realm=\"netrack\"")
    );
    assert!(body.contains("unauthorized"));

    // 2. Invalid Basic Auth -> 401
    let bad_cred = encode_base64(b"any:wrong");
    let (status, headers, _) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {bad_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        headers
            .get(header::WWW_AUTHENTICATE)
            .and_then(|h| h.to_str().ok()),
        Some("Basic realm=\"netrack\"")
    );

    // 3. Valid Basic Auth -> 200 OK
    let good_cred = encode_base64(b"netrack:mypassword");
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            None,
            Some(&format!("Basic {good_cred}")),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));

    // 4. Valid session cookie -> 200 OK
    let (token, _) = create_session(&app.db).await.unwrap();
    let (status, _, body) = app
        .get(
            "/api/prometheus/metrics",
            Some(&format!("netrack_session={token}")),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("netrack_ping_latency_ms"));
}

#[tokio::test]
async fn prometheus_metrics_24h_aggregates() {
    let app = TestApp::new().await;
    let now = Utc::now();
    for (dl, err, h) in [
        (100.0, false, 2),
        (200.0, false, 3),
        (0.0, true, 1),
        (900.0, false, 30),
    ] {
        let metrics = if err {
            Err("boom".into())
        } else {
            Ok(Metrics {
                ping: 10.0,
                jitter: 1.0,
                download: dl,
                upload: 50.0,
            })
        };
        insert(
            &app.db,
            NewSpeedtest {
                kind: TestKind::Auto,
                provider: "dummy".into(),
                server_id: None,
                server_name: None,
                server_host: None,
                result_id: None,
                metrics,
                duration: 1.0,
            },
            now - chrono::Duration::hours(h),
        )
        .await
        .unwrap();
    }
    let (_, _, body) = app.get("/api/prometheus/metrics", None, None).await;
    assert!(body.contains("netrack_download_avg_24h_mbps 150\n"));
    assert!(body.contains("netrack_tests_24h{result=\"ok\"} 2\n"));
    assert!(body.contains("netrack_tests_24h{result=\"failed\"} 1\n"));
}

#[tokio::test]
async fn prometheus_metrics_24h_empty_has_no_samples() {
    let app = TestApp::new().await;
    let (_, _, body) = app.get("/api/prometheus/metrics", None, None).await;
    for n in [
        "netrack_download_avg_24h_mbps",
        "netrack_upload_avg_24h_mbps",
        "netrack_ping_avg_24h_ms",
        "netrack_tests_24h",
    ] {
        assert!(body.contains(&format!("# HELP {n} ")));
        assert!(body.contains(&format!("# TYPE {n} gauge")));
        assert!(
            !body
                .lines()
                .any(|l| l.starts_with(n) && !l.starts_with('#')),
            "{n}"
        );
    }
}
