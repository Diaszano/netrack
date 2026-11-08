use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::routing::get;
use axum::{Router, serve};
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::db::config::{Provider, load};
use netrack::db::tests_repo::TestKind;
use netrack::measure::ThroughputConfig;
use netrack::providers::ookla::{Ookla, OoklaServer};
use netrack::providers::{ProviderError, RunOptions, SpeedProvider};
use netrack::runner::Runner;
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::sync::watch;
use tower::ServiceExt;

struct TestContext {
    app: Router,
    db: Db,
    _data_dir: TempDir,
    state: AppState,
}

async fn setup_app() -> TestContext {
    let data_dir = TempDir::new().unwrap();
    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    let runner = Runner::with_default_providers(db.clone());
    let (tx, rx) = watch::channel(());
    let scheduler = Scheduler::spawn(
        db.clone(),
        runner.clone(),
        rx,
        tempfile::tempdir().unwrap().keep(),
    );
    let state = AppState::new(db.clone(), runner.clone(), tx, Arc::new(scheduler))
        .with_data_dir(data_dir.path());
    let app = router(state.clone());
    TestContext {
        app,
        db,
        _data_dir: data_dir,
        state,
    }
}

async fn call(app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            req = req.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn create_mock_tarball(script_content: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    {
        let mut tar = tar::Builder::new(&mut gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(script_content.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "speedtest", script_content)
            .unwrap();
        tar.finish().unwrap();
    }
    gz.finish().unwrap()
}

fn write_mock_script(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
    let mut perms = std::fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms).unwrap();
}

#[tokio::test]
async fn ookla_status_defaults_to_unconsented_and_not_installed() {
    let ctx = setup_app().await;
    let (status, body) = call(&ctx.app, Method::GET, "/api/system/ookla/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["installed"], false);
    assert_eq!(body["consented"], false);
    assert_eq!(body["arch"], std::env::consts::ARCH);
}

#[tokio::test]
async fn ookla_install_fails_without_consent() {
    let ctx = setup_app().await;
    let (status, body) = call(&ctx.app, Method::POST, "/api/system/ookla/install", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("license"));
}

#[tokio::test]
async fn ookla_consent_records_license_agreement() {
    let ctx = setup_app().await;
    let (status, body) = call(&ctx.app, Method::POST, "/api/system/ookla/consent", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], true);

    let (status, status_body) = call(&ctx.app, Method::GET, "/api/system/ookla/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_body["consented"], true);
    assert_eq!(status_body["installed"], false);

    let cfg = load(&ctx.db).await.unwrap();
    assert!(cfg.ookla_license_agreed);
}

#[tokio::test]
async fn ookla_install_downloads_and_unpacks_tgz() {
    let script = "#!/bin/sh\necho '{\"type\":\"result\"}'\n";
    let tarball_bytes = create_mock_tarball(script.as_bytes());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let tarball_bytes_clone = tarball_bytes.clone();
    tokio::spawn(async move {
        let mock_app = Router::new().route(
            "/speedtest.tgz",
            get(|| async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/gzip")],
                    tarball_bytes_clone,
                )
            }),
        );
        serve(listener, mock_app).await.unwrap();
    });

    let data_dir = TempDir::new().unwrap();
    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    let runner = Runner::with_default_providers(db.clone());
    let (tx, rx) = watch::channel(());
    let scheduler = Scheduler::spawn(
        db.clone(),
        runner.clone(),
        rx,
        tempfile::tempdir().unwrap().keep(),
    );
    let state = AppState::new(db.clone(), runner.clone(), tx, Arc::new(scheduler))
        .with_data_dir(data_dir.path())
        .with_ookla_download_url(format!("http://127.0.0.1:{port}/speedtest.tgz"));
    let app = router(state.clone());

    // Agree to consent first
    let (status, _) = call(&app, Method::POST, "/api/system/ookla/consent", None).await;
    assert_eq!(status, StatusCode::OK);

    // Call install
    let (status, body) = call(&app, Method::POST, "/api/system/ookla/install", None).await;
    assert_eq!(status, StatusCode::OK, "install failed: {body:?}");
    assert_eq!(body["ok"], true);

    // Check status
    let (status, body) = call(&app, Method::GET, "/api/system/ookla/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["installed"], true);
    assert_eq!(body["consented"], true);

    // Verify binary on disk
    let bin_path = data_dir.path().join("bin").join("speedtest");
    assert!(bin_path.is_file());
    let perms = std::fs::metadata(&bin_path).unwrap().permissions();
    assert_ne!(perms.mode() & 0o111, 0, "binary must be executable");
}

#[tokio::test]
async fn ookla_servers_listing_when_not_installed_returns_404() {
    let ctx = setup_app().await;
    let (status, _) = call(&ctx.app, Method::GET, "/api/system/servers/ookla", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn ookla_servers_listing_when_installed_parses_servers() {
    let ctx = setup_app().await;
    let bin_path = ctx.state.ookla_bin_path();
    let mock_script = r#"#!/bin/sh
cat << 'EOF'
{
  "type": "servers",
  "servers": [
    {
      "id": 12345,
      "name": "São Paulo Server",
      "location": "São Paulo",
      "country": "Brazil",
      "host": "sp.speedtest.net:8080"
    },
    {
      "id": 67890,
      "name": "Rio Server",
      "location": "Rio de Janeiro",
      "country": "Brazil",
      "host": "rio.speedtest.net:8080"
    }
  ]
}
EOF
"#;
    write_mock_script(&bin_path, mock_script);

    let (status, body) = call(&ctx.app, Method::GET, "/api/system/servers/ookla", None).await;
    assert_eq!(status, StatusCode::OK, "unexpected error: {body:?}");
    let servers: Vec<OoklaServer> = serde_json::from_value(body).unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0].id, 12345);
    assert_eq!(servers[0].name, "São Paulo Server");
    assert_eq!(servers[0].location.as_deref(), Some("São Paulo"));
    assert_eq!(servers[0].country.as_deref(), Some("Brazil"));
    assert_eq!(servers[0].host.as_deref(), Some("sp.speedtest.net:8080"));
    assert_eq!(servers[1].id, 67890);
}

#[tokio::test]
async fn ookla_provider_executes_and_parses_measurement() {
    let tmp = TempDir::new().unwrap();
    let bin_path = tmp.path().join("speedtest");
    let mock_script = r#"#!/bin/sh
cat << 'EOF'
{
  "type": "result",
  "timestamp": "2026-11-07T12:00:00Z",
  "ping": {
    "jitter": 1.25,
    "latency": 15.5
  },
  "download": {
    "bandwidth": 12500000,
    "bytes": 50000000,
    "elapsed": 4000
  },
  "upload": {
    "bandwidth": 6250000,
    "bytes": 25000000,
    "elapsed": 4000
  },
  "server": {
    "id": 12345,
    "name": "Test Server",
    "location": "Curitiba",
    "country": "Brazil",
    "host": "speedtest.curitiba.net",
    "port": 8080
  },
  "result": {
    "id": "result-abc-123",
    "url": "https://www.speedtest.net/result/c/result-abc-123"
  }
}
EOF
"#;
    write_mock_script(&bin_path, mock_script);

    let provider = Ookla::new(bin_path, Some(12345));
    assert_eq!(provider.name(), "ookla");

    let opts = RunOptions {
        local_addr: Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100))),
        throughput: ThroughputConfig::default(),
    };

    let m = provider.run(&opts).await.unwrap();
    assert_eq!(m.ping, 15.5);
    assert_eq!(m.jitter, 1.25);
    // 12_500_000 * 8 / 1_000_000 = 100.0 Mbps
    assert_eq!(m.download, 100.0);
    // 6_250_000 * 8 / 1_000_000 = 50.0 Mbps
    assert_eq!(m.upload, 50.0);
    assert_eq!(m.server_id.as_deref(), Some("12345"));
    assert_eq!(m.server_name.as_deref(), Some("Test Server"));
    assert_eq!(
        m.server_host.as_deref(),
        Some("speedtest.curitiba.net:8080")
    );
    assert_eq!(m.result_id.as_deref(), Some("result-abc-123"));
}

#[tokio::test]
async fn ookla_provider_execution_failure_returns_provider_error() {
    let tmp = TempDir::new().unwrap();
    let bin_path = tmp.path().join("speedtest");
    let mock_script = "#!/bin/sh\necho 'Configuration error' >&2\nexit 1\n";
    write_mock_script(&bin_path, mock_script);

    let provider = Ookla::new(bin_path, None);
    let opts = RunOptions {
        local_addr: None,
        throughput: ThroughputConfig::default(),
    };

    let res = provider.run(&opts).await;
    assert!(res.is_err());
    let err = res.unwrap_err();
    assert!(matches!(
        err,
        ProviderError::Network(_) | ProviderError::Binary(_)
    ));
}

#[tokio::test]
async fn ookla_runner_integration_flow() {
    let tmp = TempDir::new().unwrap();
    let bin_path = tmp.path().join("bin").join("speedtest");
    let mock_script = r#"#!/bin/sh
cat << 'EOF'
{
  "type": "result",
  "ping": { "jitter": 2.0, "latency": 10.0 },
  "download": { "bandwidth": 25000000, "bytes": 100000000, "elapsed": 4000 },
  "upload": { "bandwidth": 12500000, "bytes": 50000000, "elapsed": 4000 },
  "server": { "id": 999, "name": "Integration Server", "host": "srv.net", "port": 8080 },
  "result": { "id": "res-999" }
}
EOF
"#;
    write_mock_script(&bin_path, mock_script);

    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    netrack::db::config::set(&db, "provider", "ookla")
        .await
        .unwrap();
    netrack::db::config::set(&db, "ookla_license_agreed", "true")
        .await
        .unwrap();

    let bp = bin_path.clone();
    let runner = Runner::new(
        db.clone(),
        Arc::new(move |cfg| Box::new(Ookla::new(bp.clone(), cfg.ookla_server_id))),
    );

    let id = runner.run(TestKind::Custom).await.unwrap();
    let test = netrack::db::tests_repo::get(&db, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(test.provider, "ookla");
    assert_eq!(test.server_id.as_deref(), Some("999"));
    assert_eq!(test.server_name.as_deref(), Some("Integration Server"));
    assert_eq!(test.download, Some(200.0));
    assert_eq!(test.upload, Some(100.0));
    assert_eq!(test.ping, Some(10.0));
    assert_eq!(test.jitter, Some(2.0));
}

#[tokio::test]
async fn config_supports_ookla_provider_and_server_id() {
    let ctx = setup_app().await;
    let (status, _) = call(
        &ctx.app,
        Method::PATCH,
        "/api/config/provider",
        Some(json!({ "value": "ookla" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(
        &ctx.app,
        Method::PATCH,
        "/api/config/ookla_server_id",
        Some(json!({ "value": "54321" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let cfg = load(&ctx.db).await.unwrap();
    assert_eq!(cfg.provider, Provider::Ookla);
    assert_eq!(cfg.ookla_server_id, Some(54321));
}
