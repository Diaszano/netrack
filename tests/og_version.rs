use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::get;
use chrono::Utc;
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::auth::create_session;
use netrack::db::Db;
use netrack::db::tests_repo::{Metrics, NewSpeedtest, TestKind, insert};
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use netrack::settings::Settings;
use serde_json::{Value, json};
use tokio::net::TcpListener;
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
}

impl TestApp {
    async fn new() -> Self {
        Self::with_options(false, None, None).await
    }

    async fn with_options(
        preview_mode: bool,
        preview_message: Option<String>,
        release_url: Option<String>,
    ) -> Self {
        let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
        let factory: ProviderFactory = Arc::new(|_| Box::new(DummyProvider));
        let runner = Runner::new(db.clone(), factory);
        let (tx, rx) = watch::channel(());
        let scheduler = Scheduler::spawn(
            db.clone(),
            runner.clone(),
            rx,
            tempfile::tempdir().unwrap().keep(),
        );
        let mut state = AppState::new(db.clone(), runner, tx, Arc::new(scheduler))
            .with_preview(preview_mode, preview_message);
        if let Some(url) = release_url {
            state = state.with_release_url(url);
        }
        let app = router(state);
        Self { app, db }
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

    async fn post_json(&self, uri: &str, body: Value, cookie: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }

        let body_str = serde_json::to_string(&body).unwrap();
        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body_str)).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    async fn patch_json(
        &self,
        uri: &str,
        body: Value,
        cookie: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(Method::PATCH)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }

        let body_str = serde_json::to_string(&body).unwrap();
        let res = self
            .app
            .clone()
            .oneshot(req.body(Body::from(body_str)).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }
}

// ---------------------------------------------------------------------------
// 1. Open Graph Image Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_og_image_empty_database() {
    let app = TestApp::new().await;
    let (status, headers, body) = app.get("/api/opengraph/image", None).await;

    assert_eq!(status, StatusCode::OK);
    let content_type = headers
        .get("content-type")
        .expect("content-type header")
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("image/svg+xml"),
        "expected image/svg+xml, got {content_type}"
    );

    assert!(body.contains("<svg"), "body should be SVG");
    assert!(body.to_lowercase().contains("netrack"));
}

#[tokio::test]
async fn test_og_image_with_latest_metrics() {
    let app = TestApp::new().await;

    insert(
        &app.db,
        NewSpeedtest {
            kind: TestKind::Auto,
            provider: "cloudflare".to_string(),
            server_id: Some("1".to_string()),
            server_name: Some("Cloudflare SP".to_string()),
            server_host: Some("speed.cloudflare.com".to_string()),
            result_id: None,
            metrics: Ok(Metrics {
                ping: 15.42,
                jitter: 1.23,
                download: 485.67,
                upload: 210.89,
            }),
            duration: 12.0,
        },
        Utc::now(),
    )
    .await
    .unwrap();

    let (status, headers, body) = app.get("/api/opengraph/image", None).await;
    assert_eq!(status, StatusCode::OK);
    let content_type = headers.get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.contains("image/svg+xml"));

    assert!(body.contains("<svg"));
    assert!(body.contains("485.7") || body.contains("485.67") || body.contains("485"));
    assert!(body.contains("210.9") || body.contains("210.89") || body.contains("210"));
    assert!(body.contains("15.4") || body.contains("15.42") || body.contains("15"));
    assert!(body.contains("Mbps"));
    assert!(body.contains("ms"));
}

#[tokio::test]
async fn test_og_image_auth_modes() {
    let app = TestApp::new().await;

    // 1. None mode: accessible without auth
    let (status, _, _) = app.get("/api/opengraph/image", None).await;
    assert_eq!(status, StatusCode::OK);

    // 2. Public mode: accessible without auth
    let (status, _) = app
        .post_json(
            "/api/auth/password",
            json!({ "password": "supersecretpassword", "mode": "public" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = app.get("/api/opengraph/image", None).await;
    assert_eq!(status, StatusCode::OK);

    // 3. Protected mode: blocked without auth, permitted with session
    let (token, _) = create_session(&app.db).await.unwrap();
    let auth_cookie = format!("netrack_session={token}");
    let (status, _) = app
        .post_json(
            "/api/auth/password",
            json!({ "password": "supersecretpassword", "mode": "protected" }),
            Some(&auth_cookie),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = app.get("/api/opengraph/image", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (token_after, _) = create_session(&app.db).await.unwrap();
    let valid_cookie = format!("netrack_session={token_after}");
    let (status, _, _) = app.get("/api/opengraph/image", Some(&valid_cookie)).await;
    assert_eq!(status, StatusCode::OK);
}

// ---------------------------------------------------------------------------
// 2. Preview Mode Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_preview_mode_disabled_by_default() {
    let app = TestApp::new().await;

    let (status, _, body) = app.get("/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["preview_mode"], false);

    // Patching config works when preview mode is disabled
    let (status, _) = app
        .patch_json("/api/config/cron", json!({ "value": "0 * * * *" }), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn test_preview_mode_blocks_mutations_with_custom_message() {
    let custom_msg = "Apenas demonstração. Nenhuma alteração é salva.";
    let app = TestApp::with_options(true, Some(custom_msg.to_string()), None).await;

    // Check system status surfaces preview mode and message
    let (status, _, body) = app.get("/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["preview_mode"], true);
    assert_eq!(v["preview_message"], custom_msg);

    // Read operations still work
    let (status, _, _) = app.get("/api/speedtests", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app.get("/api/config", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = app.get("/api/opengraph/image", None).await;
    assert_eq!(status, StatusCode::OK);

    // Mutations are blocked with 403 Forbidden
    let (status, err) = app
        .patch_json("/api/config/cron", json!({ "value": "0 * * * *" }), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], custom_msg);

    let (status, err) = app
        .post_json(
            "/api/auth/password",
            json!({ "new_password": "secret" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], custom_msg);

    let (status, err) = app.post_json("/api/data/reset", json!({}), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], custom_msg);

    let (status, err) = app
        .post_json(
            "/api/nodes",
            json!({ "name": "Node 1", "url": "http://127.0.0.1:5000" }),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], custom_msg);

    let (status, err) = app.post_json("/api/speedtests/run", json!({}), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(err["error"], custom_msg);

    // Login is NOT blocked with preview 403 (it performs authentication)
    let (status, _) = app
        .post_json("/api/auth/login", json!({ "password": "wrong" }), None)
        .await;
    assert_ne!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_preview_mode_default_message() {
    let app = TestApp::with_options(true, None, None).await;

    let (status, err) = app.post_json("/api/data/reset", json!({}), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        err["error"]
            .as_str()
            .unwrap()
            .contains("Demonstração ativa"),
        "expected default preview message, got: {err:?}"
    );
}

#[test]
fn test_settings_preview_mode_parsing() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert("PREVIEW_MODE".into(), "true".into());
    map.insert("PREVIEW_MESSAGE".into(), "Custom Message".into());
    let s = Settings::from_map(&map).unwrap();
    assert!(s.preview_mode);
    assert_eq!(s.preview_message.as_deref(), Some("Custom Message"));

    let mut map = HashMap::new();
    map.insert("PREVIEW_MODE".into(), "1".into());
    let s = Settings::from_map(&map).unwrap();
    assert!(s.preview_mode);
    assert_eq!(
        s.preview_message.as_deref(),
        Some("Demonstração ativa. Modificações estão desabilitadas.")
    );

    let mut map = HashMap::new();
    map.insert("PREVIEW_MODE".into(), "false".into());
    let s = Settings::from_map(&map).unwrap();
    assert!(!s.preview_mode);
    assert_eq!(s.preview_message, None);

    let mut map = HashMap::new();
    map.insert("PREVIEW_MODE".into(), "invalid".into());
    assert!(Settings::from_map(&map).is_err());
}

// ---------------------------------------------------------------------------
// 3. Version Check Tests
// ---------------------------------------------------------------------------

async fn spawn_mock_github(
    release_tag: &'static str,
    call_count: Arc<AtomicUsize>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let count = call_count.clone();
    let app = Router::new().route(
        "/repos/Diaszano/netrack/releases/latest",
        get(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                axum::Json(json!({
                    "tag_name": release_tag,
                    "name": release_tag,
                }))
            }
        }),
    );

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (
        format!("http://{addr}/repos/Diaszano/netrack/releases/latest"),
        handle,
    )
}

#[tokio::test]
async fn test_version_check_update_available() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (url, handle) = spawn_mock_github("v99.0.0", calls.clone()).await;

    let app = TestApp::with_options(false, None, Some(url)).await;
    let (status, _, body) = app.get("/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);

    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["latest_version"], "99.0.0");
    assert_eq!(v["update_available"], true);

    // Call again to verify in-memory cache prevents second call
    let (status, _, _) = app.get("/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    handle.abort();
}

#[tokio::test]
async fn test_version_check_up_to_date() {
    let calls = Arc::new(AtomicUsize::new(0));
    let (url, handle) = spawn_mock_github("v0.1.0", calls).await;

    let app = TestApp::with_options(false, None, Some(url)).await;
    let (status, _, body) = app.get("/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);

    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["latest_version"], "0.1.0");
    assert_eq!(v["update_available"], false);

    handle.abort();
}

#[tokio::test]
async fn test_version_check_offline_fails_silently() {
    // Unreachable port / connection failure
    let url = "http://127.0.0.1:1/repos/Diaszano/netrack/releases/latest".to_string();
    let app = TestApp::with_options(false, None, Some(url)).await;

    let (status, _, body) = app.get("/api/system/status", None).await;
    // Status endpoint MUST NOT fail when version check fails
    assert_eq!(status, StatusCode::OK);

    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["latest_version"], Value::Null);
    assert_eq!(v["update_available"], false);
}
