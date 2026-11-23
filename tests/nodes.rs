use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

#[allow(dead_code)]
#[derive(Clone, Debug)]
struct RecordedRequest {
    method: String,
    uri: String,
    headers: HashMap<String, String>,
    body: String,
}

struct MockNodeServer {
    port: u16,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    logins: Arc<AtomicUsize>,
}

impl MockNodeServer {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let reqs = requests.clone();
        let logins = Arc::new(AtomicUsize::new(0));
        let login_count = logins.clone();

        let app = Router::new()
            .route(
                "/api/auth/login",
                axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
                    let login_count = login_count.clone();
                    async move {
                        if body["password"] != "proxy-secret-token" {
                            return (StatusCode::UNAUTHORIZED, axum::Json(json!({})));
                        }
                        let n = login_count.fetch_add(1, Ordering::SeqCst) + 1;
                        (
                            StatusCode::OK,
                            axum::Json(json!({ "token": format!("mock-session-{n}") })),
                        )
                    }
                }),
            )
            .route(
                // Rejects the first session, as if it had expired on the node.
                "/api/expiring",
                axum::routing::get(|headers: axum::http::HeaderMap| async move {
                    match headers.get("authorization").and_then(|v| v.to_str().ok()) {
                        Some("Bearer mock-session-1") | None => StatusCode::UNAUTHORIZED,
                        Some(_) => StatusCode::OK,
                    }
                }),
            )
            .route(
                "/api/system/status",
                axum::routing::get(|| async {
                    axum::Json(json!({
                        "version": "1.2.3",
                        "uptimeSecs": 420
                    }))
                }),
            )
            .route(
                "/api/speedtests",
                axum::routing::get(|| async {
                    axum::Json(json!([
                        {
                            "id": 10,
                            "createdAt": 1700000000000i64,
                            "kind": "auto",
                            "provider": "librespeed",
                            "download": 120.5,
                            "upload": 45.2,
                            "ping": 14.1
                        }
                    ]))
                }),
            )
            .route(
                "/api/cookie",
                axum::routing::get(|| async {
                    ([("set-cookie", "netrack_session=remote; Path=/")], "ok")
                }),
            )
            .fallback(move |req: axum::extract::Request| {
                let reqs = reqs.clone();
                async move {
                    let method = req.method().to_string();
                    let uri = req.uri().to_string();
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
                    reqs.lock().unwrap().push(RecordedRequest {
                        method: method.clone(),
                        uri: uri.clone(),
                        headers,
                        body: body_str.clone(),
                    });

                    if uri.starts_with("/api/echo") {
                        let resp = axum::response::Response::builder()
                            .status(StatusCode::OK)
                            .header("x-mock-echo", "active");
                        return resp.body(Body::from(body_str)).unwrap();
                    }

                    if uri.starts_with("/api/trigger") && method == "POST" {
                        return axum::response::Response::builder()
                            .status(StatusCode::ACCEPTED)
                            .header("content-type", "application/json")
                            .body(Body::from(r#"{"status":"triggered"}"#))
                            .unwrap();
                    }

                    axum::response::Response::builder()
                        .status(StatusCode::OK)
                        .header("content-type", "application/json")
                        .body(Body::from(r#"{"ok":true}"#))
                        .unwrap()
                }
            });

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self {
            port,
            requests,
            logins,
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn recorded_requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().unwrap().clone()
    }
}

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

#[allow(dead_code)]
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

    async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        cookie: Option<&str>,
        auth_header: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        if let Some(a) = auth_header {
            req = req.header("authorization", a);
        }
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
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }

    async fn request_raw(
        &self,
        method: Method,
        uri: &str,
        body: Body,
        content_type: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(ct) = content_type {
            req = req.header("content-type", ct);
        }
        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, bytes.to_vec())
    }
}

#[tokio::test]
async fn test_node_crud_and_validation() {
    let app = TestApp::new().await;

    // 1. Initial list is empty
    let (status, _, json) = app
        .request(Method::GET, "/api/nodes", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json.as_array().unwrap().len(), 0);

    // 2. Reject empty name
    let (status, _, err) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "   ",
                "url": "http://192.168.1.100:3000",
                "password": "secret"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("name"));

    // 3. Reject invalid URL (no http/https)
    let (status, _, err) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "My Node",
                "url": "ftp://192.168.1.100:3000",
                "password": "secret"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(err["error"].as_str().unwrap().contains("http"));

    // 4. Create node with password
    let (status, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Node Alpha",
                "url": "http://192.168.1.100:3000/",
                "password": "alpha-secret-password"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("node id must be number");
    assert_eq!(created["name"], "Node Alpha");
    assert_eq!(created["url"], "http://192.168.1.100:3000"); // Trailing slash stripped
    assert_eq!(created["hasPassword"], true);
    // Password must be masked/omitted from JSON response
    assert!(created.get("password").is_none() || created["password"].is_null());

    // 5. GET single node
    let (status, _, fetched) = app
        .request(Method::GET, &format!("/api/nodes/{id}"), None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(fetched["id"], id);
    assert_eq!(fetched["name"], "Node Alpha");
    assert_eq!(fetched["hasPassword"], true);
    assert!(fetched["createdAt"].is_number());
    assert!(fetched.get("has_password").is_none());
    assert!(fetched.get("created_at").is_none());
    assert!(fetched.get("password").is_none() || fetched["password"].is_null());

    // 6. Update node name and clear password
    let (status, _, updated) = app
        .request(
            Method::PATCH,
            &format!("/api/nodes/{id}"),
            Some(json!({
                "name": "Node Alpha Renamed",
                "password": null
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "Node Alpha Renamed");
    assert_eq!(updated["hasPassword"], false);

    // 7. Update via PUT with new password and URL
    let (status, _, updated_put) = app
        .request(
            Method::PUT,
            &format!("/api/nodes/{id}"),
            Some(json!({
                "name": "Node Alpha PUT",
                "url": "https://node-alpha.example.com",
                "password": "new-put-password"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_put["name"], "Node Alpha PUT");
    assert_eq!(updated_put["url"], "https://node-alpha.example.com");
    assert_eq!(updated_put["hasPassword"], true);

    // 8. Delete node
    let (status, _, _) = app
        .request(
            Method::DELETE,
            &format!("/api/nodes/{id}"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 9. Node now 404
    let (status, _, _) = app
        .request(Method::GET, &format!("/api/nodes/{id}"), None, None, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_node_status_check() {
    let mock = MockNodeServer::start().await;
    let app = TestApp::new().await;

    // Register node pointing to mock server
    let (status, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Online Node",
                "url": mock.url(),
                "password": "node-auth-token"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();

    // Query status
    let (status, _, res) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/status"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(res["online"], true);
    assert_eq!(res["version"], "1.2.3");
    assert!(res["latestTest"].is_object());
    assert_eq!(res["latestTest"]["download"], 120.5);
    assert!(res["error"].is_null());

    // Register offline node (unreachable port)
    let (_status, _, offline_node) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Offline Node",
                "url": "http://127.0.0.1:1"
            })),
            None,
            None,
        )
        .await;
    let offline_id = offline_node["id"].as_i64().unwrap();

    // Query offline node status
    let (status, _, offline_res) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{offline_id}/status"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(offline_res["online"], false);
    assert!(offline_res["version"].is_null());
    assert!(offline_res["latestTest"].is_null());
    assert!(offline_res["error"].is_string());

    // Status on non-existent node is 404
    let (status, _, _) = app
        .request(Method::GET, "/api/nodes/9999/status", None, None, None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_transparent_proxy_get_and_post() {
    let mock = MockNodeServer::start().await;
    let app = TestApp::new().await;

    // Register node with credentials
    let (status, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Proxy Node",
                "url": mock.url(),
                "password": "proxy-secret-token"
            })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_i64().unwrap();

    // 1. GET request proxied to /api/speedtests?limit=10
    let (status, _headers, json) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/proxy/speedtests?limit=10"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.is_array());
    assert_eq!(json[0]["id"], 10);

    // 1b. GET request with optional /api/ prefix in proxy subpath routes to /api/speedtests
    let (status, _headers, json_api_prefix) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/proxy/api/speedtests?limit=10"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(json_api_prefix.is_array());
    assert_eq!(json_api_prefix[0]["id"], 10);

    // 2. POST request proxied to /api/trigger with body
    let (status, _headers, json) = app
        .request(
            Method::POST,
            &format!("/api/nodes/{id}/proxy/trigger"),
            Some(json!({ "action": "test_run" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(json["status"], "triggered");

    // 3. Raw echo streaming proxy (verifying body streaming both directions)
    let payload = b"hello transparent proxy stream!".to_vec();
    let (status, headers, body_bytes) = app
        .request_raw(
            Method::POST,
            &format!("/api/nodes/{id}/proxy/echo"),
            Body::from(payload.clone()),
            Some("text/plain"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers.get("x-mock-echo").unwrap(), "active");
    assert_eq!(body_bytes, payload);

    // 4. Verify recorded requests at mock server had credentials injected
    let reqs = mock.recorded_requests();
    assert!(!reqs.is_empty());
    for req in &reqs {
        let auth_hdr = req.headers.get("authorization");
        let cookie_hdr = req.headers.get("cookie");
        assert_eq!(
            auth_hdr.map(|s| s.as_str()),
            Some("Bearer mock-session-1"),
            "Proxy must send the session token, never the password"
        );
        assert!(
            !cookie_hdr
                .map(|s| s.as_str())
                .unwrap_or("")
                .contains("proxy-secret-token"),
            "Proxy must not leak the node password as a cookie"
        );
    }

    // 5. Proxy 404 when node does not exist
    let (status, _, _) = app
        .request(
            Method::GET,
            "/api/nodes/99999/proxy/speedtests",
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 6. Proxy 502 when target node is offline
    let (_status, _, offline_node) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Offline Proxy Node",
                "url": "http://127.0.0.1:1"
            })),
            None,
            None,
        )
        .await;
    let offline_id = offline_node["id"].as_i64().unwrap();
    let (status, _, err) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{offline_id}/proxy/speedtests"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(err["error"].as_str().unwrap().contains("proxy"));
}

#[tokio::test]
async fn proxy_strips_remote_set_cookie() {
    let mock = MockNodeServer::start().await;
    let app = TestApp::new().await;
    let (_, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({ "name": "Cookie Node", "url": mock.url() })),
            None,
            None,
        )
        .await;
    let id = created["id"].as_i64().unwrap();

    let (status, headers, _) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/proxy/cookie"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(headers.get("set-cookie").is_none());
}

#[tokio::test]
async fn proxy_to_unreachable_node_fails_fast() {
    let app = TestApp::new().await;
    let (_, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({ "name": "Dead Node", "url": "http://10.255.255.1:9" })),
            None,
            None,
        )
        .await;
    let id = created["id"].as_i64().unwrap();

    let started = std::time::Instant::now();
    let (status, _, _) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/proxy/system/status"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(started.elapsed() < std::time::Duration::from_secs(7));
}

#[tokio::test]
async fn proxy_logs_in_again_when_the_node_session_expires() {
    let mock = MockNodeServer::start().await;
    let app = TestApp::new().await;
    let (_, _, created) = app
        .request(
            Method::POST,
            "/api/nodes",
            Some(json!({
                "name": "Expiring",
                "url": mock.url(),
                "password": "proxy-secret-token"
            })),
            None,
            None,
        )
        .await;
    let id = created["id"].as_i64().unwrap();

    let (status, _, _) = app
        .request(
            Method::GET,
            &format!("/api/nodes/{id}/proxy/expiring"),
            None,
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(mock.logins.load(Ordering::SeqCst), 2);

    // The fresh token is cached: no third login.
    app.request(
        Method::GET,
        &format!("/api/nodes/{id}/proxy/speedtests"),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(mock.logins.load(Ordering::SeqCst), 2);
}
