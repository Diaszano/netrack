use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::db::tests_repo::TestKind;
use netrack::integrations::IntegrationDispatcher;
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

#[derive(Clone, Debug)]
struct MockRequest {
    method: String,
    uri: String,
    headers: HashMap<String, String>,
    body: String,
}

struct MockServer {
    port: u16,
    requests: Arc<Mutex<Vec<MockRequest>>>,
}

impl MockServer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let reqs = requests.clone();

        let app = Router::new().fallback(move |req: axum::extract::Request| {
            let reqs = reqs.clone();
            async move {
                let (parts, body) = req.into_parts();
                let bytes = axum::body::to_bytes(body, usize::MAX)
                    .await
                    .unwrap_or_default();
                let body_str = String::from_utf8_lossy(&bytes).to_string();
                let mut headers = HashMap::new();
                for (k, v) in parts.headers.iter() {
                    if let Ok(val) = v.to_str() {
                        headers.insert(k.as_str().to_lowercase(), val.to_string());
                    }
                }
                reqs.lock().unwrap().push(MockRequest {
                    method: parts.method.to_string(),
                    uri: parts.uri.to_string(),
                    headers,
                    body: body_str,
                });
                StatusCode::OK
            }
        });

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        MockServer { port, requests }
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn recorded(&self) -> Vec<MockRequest> {
        self.requests.lock().unwrap().clone()
    }
}

struct SuccessProvider;

impl SpeedProvider for SuccessProvider {
    fn name(&self) -> &'static str {
        "success"
    }

    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move {
            Ok(Measurement {
                ping: 15.5,
                jitter: 1.2,
                download: 250.0,
                upload: 55.0,
                server_id: Some("srv-1".into()),
                server_name: Some("Test Server".into()),
                server_host: Some("srv.example.com".into()),
                result_id: Some("res-123".into()),
            })
        }
        .boxed()
    }
}

struct FailingProvider;

impl SpeedProvider for FailingProvider {
    fn name(&self) -> &'static str {
        "failing"
    }

    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move { Err(ProviderError::Network("simulated connection loss".into())) }.boxed()
    }
}

struct TestApp {
    app: Router,
    db: Db,
    runner: Runner,
    reload_tx: watch::Sender<()>,
    _bk: tempfile::TempDir,
}

impl TestApp {
    async fn new(failing: bool) -> Self {
        let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
        let factory: ProviderFactory = if failing {
            Arc::new(|_| Box::new(FailingProvider))
        } else {
            Arc::new(|_| Box::new(SuccessProvider))
        };
        let runner = Runner::new(db.clone(), factory);
        let (reload_tx, reload_rx) = watch::channel(());
        let bk = tempfile::tempdir().unwrap();
        let scheduler = Scheduler::spawn(
            db.clone(),
            runner.clone(),
            reload_rx,
            bk.path().to_path_buf(),
        );
        let state = AppState::new(
            db.clone(),
            runner.clone(),
            reload_tx.clone(),
            Arc::new(scheduler),
        );
        let app = router(state);
        Self {
            app,
            db,
            runner,
            reload_tx,
            _bk: bk,
        }
    }

    async fn request(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };

        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
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

#[tokio::test]
async fn integrations_crud_and_validation() {
    let app = TestApp::new(false).await;

    // 1. Initial GET is empty
    let (status, body) = app.request(Method::GET, "/api/integrations", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!([]));

    // 2. Invalid validation tests
    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "",
                "channel_type": "webhook",
                "config": {"url": "http://localhost"},
                "events": ["speedtest.finished"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Invalid Channel",
                "channel_type": "pigeon",
                "config": {},
                "events": ["speedtest.finished"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // 3. Create valid integration
    let (status, created) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "My Webhook",
                "channel_type": "webhook",
                "config": {"url": "http://127.0.0.1:8080/hook"},
                "events": ["speedtest.finished", "speedtest.failed"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("id must be present");
    assert_eq!(created["name"], "My Webhook");
    assert_eq!(created["channel_type"], "webhook");
    assert_eq!(created["enabled"], true);

    // 4. GET lists the created integration
    let (status, list) = app.request(Method::GET, "/api/integrations", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = list.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["id"], id);

    // 5. PUT updates the integration
    let (status, updated) = app
        .request(
            Method::PUT,
            &format!("/api/integrations/{id}"),
            Some(json!({
                "name": "My Webhook Updated",
                "channel_type": "webhook",
                "config": {"url": "http://127.0.0.1:8080/hook-new"},
                "events": ["speedtest.finished"],
                "enabled": false
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "My Webhook Updated");
    assert_eq!(updated["enabled"], false);
    assert_eq!(updated["config"]["url"], "http://127.0.0.1:8080/hook-new");

    // 6. DELETE removes the integration
    let (status, _) = app
        .request(Method::DELETE, &format!("/api/integrations/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 7. GET is empty again
    let (status, list) = app.request(Method::GET, "/api/integrations", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list, json!([]));
}

#[tokio::test]
async fn integration_test_ping_endpoint() {
    let mock = MockServer::start().await;
    let app = TestApp::new(false).await;

    // Create integration pointing to mock server
    let (status, created) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Test Webhook",
                "channel_type": "webhook",
                "config": {"url": format!("{}/webhook-test", mock.base_url())},
                "events": ["speedtest.finished"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();

    // Trigger test endpoint
    let (status, body) = app
        .request(Method::POST, &format!("/api/integrations/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);

    // Verify mock server received the request
    let recorded = mock.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].uri, "/webhook-test");
    assert_eq!(recorded[0].method, "POST");
    assert!(recorded[0].body.contains("test"));

    // Testing non-existent ID returns 404
    let (status, _) = app
        .request(Method::POST, "/api/integrations/99999/test", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Testing all 8 channels via the test endpoint
    let channels = vec![
        (
            "discord",
            json!({"webhook_url": format!("{}/test-discord", mock.base_url())}),
        ),
        (
            "telegram",
            json!({"bot_token": "token123", "chat_id": "chat123", "api_url": mock.base_url()}),
        ),
        (
            "pushover",
            json!({"user_key": "user123", "api_token": "token123", "api_url": mock.base_url()}),
        ),
        (
            "gotify",
            json!({"server_url": mock.base_url(), "app_token": "token123"}),
        ),
        (
            "ntfy",
            json!({"server_url": mock.base_url(), "topic": "test-topic"}),
        ),
        (
            "webhook",
            json!({"url": format!("{}/test-webhook-2", mock.base_url())}),
        ),
        (
            "healthchecks",
            json!({"ping_url": format!("{}/test-hc", mock.base_url())}),
        ),
        (
            "influxdb",
            json!({"url": mock.base_url(), "org": "o", "bucket": "b"}),
        ),
    ];

    for (channel_type, config) in channels {
        let (status, item) = app
            .request(
                Method::POST,
                "/api/integrations",
                Some(json!({
                    "name": format!("Test {channel_type}"),
                    "channel_type": channel_type,
                    "config": config,
                    "events": ["speedtest.finished"],
                    "enabled": true
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let id = item["id"].as_i64().unwrap();

        let (status, body) = app
            .request(Method::POST, &format!("/api/integrations/{id}/test"), None)
            .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "Testing channel {channel_type} should succeed"
        );
        assert_eq!(body["success"], true);
    }
}

#[tokio::test]
async fn dispatcher_handles_all_channel_types_on_runner_success() {
    let mock = MockServer::start().await;
    let app = TestApp::new(false).await;

    // Spawn dispatcher
    let client = reqwest::Client::new();
    let dispatcher = IntegrationDispatcher::new(
        app.db.clone(),
        app.runner.subscribe(),
        app.reload_tx.subscribe(),
        client,
    );
    dispatcher.spawn();

    let base = mock.base_url();

    // Register all 8 channel types
    let configs = vec![
        ("discord", json!({"webhook_url": format!("{base}/discord")})),
        (
            "telegram",
            json!({"bot_token": "token123", "chat_id": "chat123", "api_url": base}),
        ),
        (
            "pushover",
            json!({"user_key": "user123", "api_token": "token123", "api_url": base}),
        ),
        (
            "gotify",
            json!({"server_url": base, "app_token": "token123"}),
        ),
        (
            "ntfy",
            json!({"server_url": base, "topic": "speedtest-topic"}),
        ),
        ("webhook", json!({"url": format!("{base}/webhook")})),
        (
            "healthchecks",
            json!({"ping_url": format!("{base}/hc/uuid123")}),
        ),
        (
            "influxdb",
            json!({"url": base, "org": "my-org", "bucket": "my-bucket"}),
        ),
    ];

    for (channel_type, config) in configs {
        let (status, _) = app
            .request(
                Method::POST,
                "/api/integrations",
                Some(json!({
                    "name": format!("Integration {channel_type}"),
                    "channel_type": channel_type,
                    "config": config,
                    "events": ["speedtest.started", "speedtest.finished"],
                    "enabled": true
                })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // Add a disabled integration that shouldn't receive anything
    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Disabled Webhook",
                "channel_type": "webhook",
                "config": {"url": format!("{base}/disabled-webhook")},
                "events": ["speedtest.finished"],
                "enabled": false
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Run test
    app.runner.run(TestKind::Custom).await.unwrap();

    // Wait briefly for background dispatch
    tokio::time::sleep(Duration::from_millis(300)).await;

    let recorded = mock.recorded();
    assert!(!recorded.is_empty(), "Dispatcher should deliver requests");

    // Disabled integration should never have been contacted
    assert!(!recorded.iter().any(|r| r.uri.contains("disabled-webhook")));

    // Verify delivery for channels
    assert!(recorded.iter().any(|r| r.uri.contains("discord")));
    assert!(
        recorded
            .iter()
            .any(|r| r.uri.contains("/bottoken123/sendMessage"))
    );
    assert!(recorded.iter().any(|r| r.uri.contains("/1/messages.json")));
    assert!(recorded.iter().any(|r| r.uri.contains("/message")
        && r.headers.get("x-gotify-key").map(|v| v.as_str()) == Some("token123")));
    assert!(recorded.iter().any(|r| r.uri.contains("/speedtest-topic")));
    assert!(recorded.iter().any(|r| r.uri == "/webhook"));
    assert!(recorded.iter().any(|r| r.uri.starts_with("/hc/uuid123")));
    assert!(recorded.iter().any(|r| r.uri.contains("/api/v2/write")));

    // Verify payload contents in webhook finished event
    let webhook_req = recorded
        .iter()
        .find(|r| r.uri == "/webhook" && r.body.contains("speedtest.finished"))
        .unwrap();
    let body_json: Value = serde_json::from_str(&webhook_req.body).unwrap();
    assert_eq!(body_json["event"], "speedtest.finished");
    assert_eq!(body_json["data"]["metrics"]["download"], 250.0);
    assert_eq!(body_json["data"]["metrics"]["upload"], 55.0);
    assert_eq!(body_json["data"]["metrics"]["ping"], 15.5);
}

#[tokio::test]
async fn dispatcher_delivers_degraded_event_to_webhook() {
    let mock = MockServer::start().await;
    let app = TestApp::new(false).await; // download 250
    netrack::db::config::set(&app.db, "expected_download", "1000")
        .await
        .unwrap();
    let dispatcher = IntegrationDispatcher::new(
        app.db.clone(),
        app.runner.subscribe(),
        app.reload_tx.subscribe(),
        reqwest::Client::new(),
    );
    dispatcher.spawn();

    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Degraded",
                "channel_type": "webhook",
                "config": {"url": format!("{}/webhook", mock.base_url())},
                "events": ["speedtest.degraded"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    app.runner.run(TestKind::Custom).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;

    let recorded = mock.recorded();
    assert_eq!(recorded.len(), 1);
    let body: Value = serde_json::from_str(&recorded[0].body).unwrap();
    assert_eq!(body["event"], "speedtest.degraded");
    assert_eq!(body["data"]["expected"]["download"], 1000.0);
    assert_eq!(body["data"]["thresholdPct"], 70);
}

#[tokio::test]
async fn dispatcher_handles_failure_events_and_resilient_to_errors() {
    let mock = MockServer::start().await;
    let app = TestApp::new(true).await; // Failing provider

    let client = reqwest::Client::new();
    let dispatcher = IntegrationDispatcher::new(
        app.db.clone(),
        app.runner.subscribe(),
        app.reload_tx.subscribe(),
        client,
    );
    dispatcher.spawn();

    let base = mock.base_url();

    // 1. Healthchecks listening to speedtest.failed
    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Healthchecks Fail",
                "channel_type": "healthchecks",
                "config": {"ping_url": format!("{base}/hc-fail")},
                "events": ["speedtest.failed"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // 2. Integration with unreachable port (should not crash dispatcher or runner)
    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Broken Webhook",
                "channel_type": "webhook",
                "config": {"url": "http://127.0.0.1:1/nonexistent"},
                "events": ["speedtest.failed"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Run test - provider fails, runner records failed row and emits RunEvent::Failed
    let id = app.runner.run(TestKind::Custom).await.unwrap();
    assert!(id > 0);

    // Wait for dispatch
    tokio::time::sleep(Duration::from_millis(300)).await;

    let recorded = mock.recorded();
    assert!(recorded.iter().any(|r| r.uri == "/hc-fail/fail"));
}

#[tokio::test]
async fn dispatcher_handles_config_updated_event() {
    let mock = MockServer::start().await;
    let app = TestApp::new(false).await;

    let client = reqwest::Client::new();
    let dispatcher = IntegrationDispatcher::new(
        app.db.clone(),
        app.runner.subscribe(),
        app.reload_tx.subscribe(),
        client,
    );
    dispatcher.spawn();

    let base = mock.base_url();

    let (status, _) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Config Webhook",
                "channel_type": "webhook",
                "config": {"url": format!("{base}/config-changed")},
                "events": ["config.updated"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    // Trigger config reload ping
    app.reload_tx.send_replace(());

    // Wait for dispatch
    tokio::time::sleep(Duration::from_millis(300)).await;

    let recorded = mock.recorded();
    let req = recorded.iter().find(|r| r.uri == "/config-changed");
    assert!(req.is_some(), "config.updated event should be delivered");
    assert!(req.unwrap().body.contains("config.updated"));
}

#[tokio::test]
async fn test_ping_endpoint_reports_delivery_failure() {
    let app = TestApp::new(false).await;

    // Create integration pointing to non-existent / unreachable port
    let (status, created) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Unreachable Webhook",
                "channel_type": "webhook",
                "config": {"url": "http://127.0.0.1:1/unreachable"},
                "events": ["speedtest.finished"],
                "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();

    let (status, body) = app
        .request(Method::POST, &format!("/api/integrations/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(body["error"].as_str().unwrap().contains("delivery failed"));
}

#[tokio::test]
async fn integration_secrets_are_masked_and_preserved() {
    let mock = MockServer::start().await;
    let app = TestApp::new(false).await;
    let api = format!("{}/tg", mock.base_url());

    let (status, created) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "TG", "channel_type": "telegram",
                "config": {"bot_token": "abc", "chat_id": "1", "api_url": api},
                "events": ["speedtest.finished"], "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["config"]["bot_token"], "***");
    let id = created["id"].as_i64().unwrap();

    let (_, list) = app.request(Method::GET, "/api/integrations", None).await;
    assert_eq!(list[0]["config"]["bot_token"], "***");
    assert_eq!(list[0]["config"]["chat_id"], "1");

    let (status, updated) = app
        .request(
            Method::PUT,
            &format!("/api/integrations/{id}"),
            Some(json!({
                "name": "TG", "channel_type": "telegram",
                "config": {"bot_token": "***", "chat_id": "2", "api_url": api},
                "events": ["speedtest.finished"], "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["config"]["bot_token"], "***");

    let stored = netrack::integrations::get(&app.db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.config["bot_token"], "abc");
    assert_eq!(stored.config["chat_id"], "2");

    let (status, body) = app
        .request(Method::POST, &format!("/api/integrations/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["success"], true);
    assert!(mock.recorded().iter().any(|r| r.uri.contains("botabc")));

    // "***" on create has nothing to unmask and must never be stored.
    let (status, body) = app
        .request(
            Method::POST,
            "/api/integrations",
            Some(json!({
                "name": "Bad", "channel_type": "telegram",
                "config": {"bot_token": "***", "chat_id": "1"},
                "events": [], "enabled": true
            })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("bot_token"));
}
