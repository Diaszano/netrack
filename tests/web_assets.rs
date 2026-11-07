use std::sync::Arc;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, Request, StatusCode};
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use serde_json::Value;
use tokio::sync::watch;
use tower::ServiceExt;

struct FakeProvider;

impl SpeedProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move {
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

async fn setup_app() -> Router {
    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    let factory: ProviderFactory = Arc::new(|_| Box::new(FakeProvider));
    let runner = Runner::new(db.clone(), factory);
    let (tx, rx) = watch::channel(());
    let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx);
    let state = AppState::new(db.clone(), runner.clone(), tx, Arc::new(scheduler));
    router(state)
}

async fn call_raw(app: &Router, method: Method, uri: &str) -> (StatusCode, HeaderMap, Bytes) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes)
}

#[tokio::test]
async fn root_and_spa_routes_serve_index_html() {
    let app = setup_app().await;

    for path in ["/", "/statistics", "/settings"] {
        let (status, headers, body) = call_raw(&app, Method::GET, path).await;
        assert_eq!(status, StatusCode::OK, "path {path} should return 200");
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(
            content_type.starts_with("text/html"),
            "path {path} content-type should be text/html, got {content_type}"
        );
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("<div id=\"app\">"),
            "path {path} body should contain <div id=\"app\">"
        );
    }
}

#[tokio::test]
async fn static_assets_and_fonts_served_with_mime_types() {
    let app = setup_app().await;

    let (status, headers, body) = call_raw(&app, Method::GET, "/styles.css").await;
    assert_eq!(status, StatusCode::OK);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/css"),
        "styles.css should have text/css content-type, got {ct}"
    );
    assert!(!body.is_empty());

    let (status, headers, body) = call_raw(&app, Method::GET, "/app.js").await;
    assert_eq!(status, StatusCode::OK);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("text/javascript"),
        "app.js should have text/javascript content-type, got {ct}"
    );
    assert!(!body.is_empty());

    let (status, headers, body) =
        call_raw(&app, Method::GET, "/fonts/space-grotesk-latin.woff2").await;
    assert_eq!(status, StatusCode::OK);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(ct, "font/woff2");
    let cache_control = headers
        .get("cache-control")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        cache_control.contains("immutable"),
        "fonts cache-control should contain immutable, got {cache_control}"
    );
    assert!(!body.is_empty(), "font body should not be empty");
}

#[tokio::test]
async fn locales_index_lists_discovered_languages() {
    let app = setup_app().await;

    let (status, headers, body) = call_raw(&app, Method::GET, "/locales/index.json").await;
    assert_eq!(status, StatusCode::OK);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "content-type should be application/json, got {ct}"
    );

    let list: Vec<Value> = serde_json::from_slice(&body).expect("valid json array");
    let en = list
        .iter()
        .find(|item| item["code"] == "en")
        .expect("en locale present");
    assert_eq!(en["name"], "English");

    let pt_br = list
        .iter()
        .find(|item| item["code"] == "pt-BR")
        .expect("pt-BR locale present");
    assert_eq!(pt_br["name"], "Português (Brasil)");
}

#[tokio::test]
async fn spa_routes_serve_html_while_unknown_api_and_assets_404() {
    let app = setup_app().await;

    // Unknown API route must return 404 JSON error
    let (status, headers, body) = call_raw(&app, Method::GET, "/api/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "/api/nope content-type should be json, got {ct}"
    );
    let json: Value = serde_json::from_slice(&body).expect("valid json error");
    assert_eq!(json["error"], "not found");

    // Missing asset with file extension must return 404 JSON error
    let (status, headers, body) = call_raw(&app, Method::GET, "/missing.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "/missing.js content-type should be json, got {ct}"
    );
    let json: Value = serde_json::from_slice(&body).expect("valid json error");
    assert_eq!(json["error"], "not found");

    // POST / must return 405 Method Not Allowed JSON error
    let (status, headers, body) = call_raw(&app, Method::POST, "/").await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ct.starts_with("application/json"),
        "POST / content-type should be json, got {ct}"
    );
    let json: Value = serde_json::from_slice(&body).expect("valid json error");
    assert_eq!(json["error"], "method not allowed");
}
