use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use futures::FutureExt;
use futures::StreamExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::db::tests_repo::TestKind;
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, RunEvent, Runner};
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

struct FakeProvider {
    delay: Duration,
}

impl SpeedProvider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        let delay = self.delay;
        async move {
            tokio::time::sleep(delay).await;
            Ok(Measurement {
                ping: 12.0,
                jitter: 1.0,
                download: 300.0,
                upload: 50.0,
                server_id: Some("7".into()),
                server_name: Some("Porto Alegre".into()),
                server_host: Some("poa.example.com".into()),
                result_id: Some("res-1".into()),
            })
        }
        .boxed()
    }
}

struct Ctx {
    app: Router,
    runner: Runner,
    db: Db,
}

async fn setup(delay: Duration) -> Ctx {
    let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
    let factory: ProviderFactory = Arc::new(move |_| Box::new(FakeProvider { delay }));
    let runner = Runner::new(db.clone(), factory);
    let (tx, rx) = watch::channel(());
    let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx);
    let state = AppState::new(db.clone(), runner.clone(), tx, Arc::new(scheduler));
    Ctx {
        app: router(state),
        runner,
        db,
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

async fn wait_finished(rx: &mut tokio::sync::broadcast::Receiver<RunEvent>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if matches!(rx.recv().await.unwrap(), RunEvent::Finished { .. }) {
                return;
            }
        }
    })
    .await
    .expect("run finished");
}

#[tokio::test]
async fn run_then_list_returns_row() {
    let ctx = setup(Duration::ZERO).await;
    let mut rx = ctx.runner.subscribe();
    let (status, body) = call(&ctx.app, Method::POST, "/api/speedtests/run", None).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body, json!({ "status": "started" }));
    wait_finished(&mut rx).await;
    let (status, body) = call(&ctx.app, Method::GET, "/api/speedtests", None).await;
    assert_eq!(status, StatusCode::OK);
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["kind"], "custom");
    assert_eq!(rows[0]["download"], 300.0);
    let id = rows[0]["id"].as_i64().unwrap();
    let (status, _) = call(
        &ctx.app,
        Method::GET,
        &format!("/api/speedtests/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &ctx.app,
        Method::DELETE,
        &format!("/api/speedtests/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &ctx.app,
        Method::GET,
        &format!("/api/speedtests/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(
        &ctx.app,
        Method::DELETE,
        &format!("/api/speedtests/{id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn run_while_running_is_409() {
    let ctx = setup(Duration::from_millis(500)).await;
    let runner = ctx.runner.clone();
    let bg = tokio::spawn(async move { runner.run(TestKind::Auto).await });
    while !ctx.runner.is_running() {
        tokio::task::yield_now().await;
    }
    let (status, body) = call(&ctx.app, Method::POST, "/api/speedtests/run", None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body, json!({ "error": "test already running" }));
    bg.await.unwrap().unwrap();
}

#[tokio::test]
async fn patch_config_invalid_is_400_with_message() {
    let ctx = setup(Duration::ZERO).await;
    let (status, body) = call(
        &ctx.app,
        Method::PATCH,
        "/api/config/cron",
        Some(json!({ "value": "nope" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("cron"));

    let (status, body) = call(
        &ctx.app,
        Method::PATCH,
        "/api/config/bogus",
        Some(json!({ "value": "x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("bogus"));
}

#[tokio::test]
async fn patch_config_valid_is_persisted_and_pings_reload() {
    let ctx = setup(Duration::ZERO).await;
    let (status, _) = call(
        &ctx.app,
        Method::PATCH,
        "/api/config/cron",
        Some(json!({ "value": "*/5 * * * *" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = call(&ctx.app, Method::GET, "/api/config", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["cron"], "*/5 * * * *");
    let cfg = netrack::db::config::load(&ctx.db).await.unwrap();
    assert_eq!(cfg.cron, "*/5 * * * *");
}

#[tokio::test]
async fn statistics_bad_range_is_400() {
    let ctx = setup(Duration::ZERO).await;
    let (status, body) = call(
        &ctx.app,
        Method::GET,
        "/api/speedtests/statistics?from=2026-02-01&to=2026-01-01",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string());
    let (status, _) = call(
        &ctx.app,
        Method::GET,
        "/api/speedtests/statistics?from=nope&to=2026-01-01",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = call(
        &ctx.app,
        Method::GET,
        "/api/speedtests/statistics?from=2026-01-01&to=2026-01-02",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 0);
}

#[tokio::test]
async fn status_reports_running_and_paused() {
    let ctx = setup(Duration::from_millis(500)).await;
    let (status, body) = call(&ctx.app, Method::GET, "/api/speedtests/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["running"], false);
    assert_eq!(body["paused"], false);

    let runner = ctx.runner.clone();
    let bg = tokio::spawn(async move { runner.run(TestKind::Auto).await });
    while !ctx.runner.is_running() {
        tokio::task::yield_now().await;
    }
    let (_, body) = call(&ctx.app, Method::GET, "/api/speedtests/status", None).await;
    assert_eq!(body["running"], true);
    bg.await.unwrap().unwrap();

    let (status, _) = call(&ctx.app, Method::POST, "/api/speedtests/pause", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The scheduler replans asynchronously after the reload ping.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_, body) = call(&ctx.app, Method::GET, "/api/speedtests/status", None).await;
            assert_eq!(body["paused"], true);
            if body["nextRun"].is_null() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("nextRun cleared after pause");

    let (status, _) = call(&ctx.app, Method::POST, "/api/speedtests/continue", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // The scheduler replans asynchronously after the reload ping.
    let next = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let (_, body) = call(&ctx.app, Method::GET, "/api/speedtests/status", None).await;
            assert_eq!(body["paused"], false);
            if body["nextRun"].is_i64() {
                return body["nextRun"].as_i64().unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("nextRun reported after continue");
    assert!(next > chrono::Utc::now().timestamp_millis());
}

#[tokio::test]
async fn events_stream_emits_started_and_finished() {
    let ctx = setup(Duration::ZERO).await;
    let res = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/speedtests/events")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["content-type"].to_str().unwrap(),
        "text/event-stream"
    );
    let mut stream = res.into_body().into_data_stream();
    ctx.runner.run(TestKind::Custom).await.unwrap();
    let mut seen = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !(seen.contains("\"type\":\"started\"") && seen.contains("\"type\":\"finished\"")) {
            let chunk = stream.next().await.expect("stream open").unwrap();
            seen.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    })
    .await
    .unwrap_or_else(|_| panic!("events missing, got: {seen}"));
    assert!(seen.contains("data: {"));
}

#[tokio::test]
async fn healthz_ok() {
    let ctx = setup(Duration::ZERO).await;
    let res = ctx
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"ok");
}

#[tokio::test]
async fn system_status_and_interfaces() {
    let ctx = setup(Duration::ZERO).await;
    let (status, body) = call(&ctx.app, Method::GET, "/api/system/status", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["tz"], "UTC");
    assert!(body["uptimeSecs"].is_u64());
    // cpuPercent / rssBytes are null off Linux, so only the keys are checked.
    assert!(body.get("cpuPercent").is_some());
    assert!(body.get("rssBytes").is_some());
    let (status, body) = call(&ctx.app, Method::GET, "/api/system/interfaces", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.is_array());
}
