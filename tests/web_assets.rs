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

async fn setup_app() -> (Router, tempfile::TempDir) {
    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    let factory: ProviderFactory = Arc::new(|_| Box::new(FakeProvider));
    let runner = Runner::new(db.clone(), factory);
    let (tx, rx) = watch::channel(());
    let bk = tempfile::tempdir().unwrap();
    let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx, bk.path().to_path_buf());
    let state = AppState::new(db.clone(), runner.clone(), tx, Arc::new(scheduler));
    (router(state), bk)
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
    let (app, _bk) = setup_app().await;

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
    let (app, _bk) = setup_app().await;

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
    let (app, _bk) = setup_app().await;

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
    let (app, _bk) = setup_app().await;

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

fn resolve_url_path(base_url: &str, import_spec: &str) -> String {
    if import_spec.starts_with('/') {
        return import_spec.to_string();
    }
    let base_dir = match base_url.rfind('/') {
        Some(idx) => &base_url[..idx],
        None => "",
    };
    let combined = format!("{}/{}", base_dir, import_spec);
    let mut parts: Vec<&str> = Vec::new();
    for seg in combined.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    format!("/{}", parts.join("/"))
}

fn extract_static_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut rest = source;
    while let Some(import_idx) = rest.find("import ") {
        rest = &rest[import_idx + 7..];
        let semi_idx = rest.find(';').unwrap_or(rest.len());
        let stmt = &rest[..semi_idx];

        let target = if let Some(from_idx) = stmt.find("from ") {
            &stmt[from_idx + 5..]
        } else {
            stmt
        };

        if let Some(first_quote) = target.find(['"', '\'']) {
            let quote_char = target.as_bytes()[first_quote] as char;
            let after_quote = &target[first_quote + 1..];
            if let Some(end_quote) = after_quote.find(quote_char) {
                let path = &after_quote[..end_quote];
                imports.push(path.to_string());
            }
        }

        if semi_idx < rest.len() {
            rest = &rest[semi_idx + 1..];
        } else {
            break;
        }
    }
    imports
}

#[test]
fn all_es_module_imports_resolve_to_embedded_assets() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let web_dir = std::path::Path::new(manifest_dir).join("web");

    let mut js_files = Vec::new();

    // 1. web/*.js
    if let Ok(entries) = std::fs::read_dir(&web_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("js") {
                let filename = p.file_name().unwrap().to_string_lossy();
                js_files.push((format!("/{}", filename), p));
            }
        }
    }

    // 2. web/views/*.js
    let views_dir = web_dir.join("views");
    if let Ok(entries) = std::fs::read_dir(&views_dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.extension().and_then(|s| s.to_str()) == Some("js") {
                let filename = p.file_name().unwrap().to_string_lossy();
                js_files.push((format!("/views/{}", filename), p));
            }
        }
    }

    assert!(!js_files.is_empty(), "found JS files in web and web/views");

    let mut total_imports = 0;

    for (url_path, file_path) in js_files {
        let content = std::fs::read_to_string(&file_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", file_path.display()));

        let imports = extract_static_imports(&content);
        for import_specifier in imports {
            total_imports += 1;
            let resolved_url = resolve_url_path(&url_path, &import_specifier);

            let asset = netrack::web::get_asset(&resolved_url);
            assert!(
                asset.is_some(),
                "In {}, import '{}' resolved to '{}', which is missing in embedded ASSETS",
                url_path,
                import_specifier,
                resolved_url
            );
            let (ct, bytes) = asset.unwrap();
            assert!(
                ct.starts_with("text/javascript"),
                "Asset '{}' should be text/javascript, got '{}'",
                resolved_url,
                ct
            );
            assert!(
                !bytes.is_empty(),
                "Asset '{}' bytes should not be empty",
                resolved_url
            );
        }
    }

    assert!(
        total_imports >= 5,
        "Expected at least 5 module imports checked, found {total_imports}"
    );
}

#[tokio::test]
async fn html_has_security_headers() {
    let (app, _bk) = setup_app().await;
    let (_, headers, _) = call_raw(&app, Method::GET, "/").await;
    let h = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).unwrap_or("");
    assert_eq!(
        h("content-security-policy"),
        "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'"
    );
    assert_eq!(h("x-frame-options"), "DENY");
    assert_eq!(h("x-content-type-options"), "nosniff");
}
