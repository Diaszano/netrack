//! End to end: a fake OTLP/HTTP collector must receive traces, metrics and logs.
//! Own test binary because `init` installs the global subscriber and meter provider.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::Uri;
use netrack::db::tests_repo::{Metrics, TestKind};
use tokio::net::TcpListener;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exports_traces_metrics_and_logs_over_otlp_http() {
    let paths = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = paths.clone();
    let app = Router::new().fallback(move |uri: Uri| {
        let seen = seen.clone();
        async move {
            seen.lock().unwrap().push(uri.path().to_owned());
        }
    });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await });

    // SAFETY: no other thread reads the environment yet.
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", format!("http://{addr}"));
        std::env::set_var("OTEL_SERVICE_NAME", "netrack-test");
    }
    let telemetry = netrack::telemetry::init().unwrap();

    let m = Metrics {
        ping: 10.0,
        jitter: 1.0,
        download: 100.0,
        upload: 50.0,
    };
    netrack::telemetry::record_run(TestKind::Auto, "ookla", Some(&m), 12.5, false);
    tracing::info_span!("test.span").in_scope(|| tracing::warn!("test log line"));
    telemetry.shutdown();

    let mut got = paths.lock().unwrap().clone();
    got.sort();
    got.dedup();
    assert_eq!(got, ["/v1/logs", "/v1/metrics", "/v1/traces"]);
}
