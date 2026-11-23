//! OpenTelemetry export of logs, traces and metrics over OTLP/HTTP.
//!
//! Off unless an endpoint is configured with the standard `OTEL_EXPORTER_OTLP_ENDPOINT`
//! (or a per-signal `OTEL_EXPORTER_OTLP_{TRACES,METRICS,LOGS}_ENDPOINT`). Headers, timeout,
//! `OTEL_SERVICE_NAME` and `OTEL_RESOURCE_ATTRIBUTES` are read by the SDK itself.
//! Without an endpoint the instruments below are no-ops and logs stay on stdout only.

use std::sync::OnceLock;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use opentelemetry::metrics::{Counter, Gauge, Histogram};
use opentelemetry::propagation::Extractor;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::SdkMeterProvider;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, filter::filter_fn};

use crate::db::tests_repo::{Metrics, TestKind};

/// Providers to flush on exit. Dropping without [`Telemetry::shutdown`] loses the last batch.
#[derive(Default)]
pub struct Telemetry {
    traces: Option<SdkTracerProvider>,
    metrics: Option<SdkMeterProvider>,
    logs: Option<SdkLoggerProvider>,
}

/// Whether `signal` (`TRACES`, `METRICS` or `LOGS`) is exported, given an env lookup.
fn signal_enabled(get: &impl Fn(&str) -> Option<String>, signal: &str) -> bool {
    let set = |k: &str| get(k).is_some_and(|v| !v.trim().is_empty());
    if get("OTEL_SDK_DISABLED").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
        return false;
    }
    if get(&format!("OTEL_{signal}_EXPORTER")).is_some_and(|v| v.trim() == "none") {
        return false;
    }
    set("OTEL_EXPORTER_OTLP_ENDPOINT") || set(&format!("OTEL_EXPORTER_OTLP_{signal}_ENDPOINT"))
}

fn enabled(signal: &str) -> bool {
    signal_enabled(&|k| std::env::var(k).ok(), signal)
}

fn resource() -> Resource {
    let mut b = Resource::builder()
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")));
    if std::env::var_os("OTEL_SERVICE_NAME").is_none() {
        b = b.with_service_name("netrack");
    }
    b.build()
}

/// Builds the providers. Must not run on a tokio worker: the blocking HTTP client creates
/// and drops its own runtime, which panics inside another one.
fn build_providers() -> anyhow::Result<Telemetry> {
    let resource = resource();
    let mut t = Telemetry::default();
    if enabled("TRACES") {
        let exporter = SpanExporter::builder().with_http().build()?;
        t.traces = Some(
            SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource.clone())
                .build(),
        );
    }
    if enabled("METRICS") {
        let exporter = MetricExporter::builder().with_http().build()?;
        t.metrics = Some(
            SdkMeterProvider::builder()
                .with_periodic_exporter(exporter)
                .with_resource(resource.clone())
                .build(),
        );
    }
    if enabled("LOGS") {
        let exporter = LogExporter::builder().with_http().build()?;
        t.logs = Some(
            SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build(),
        );
    }
    Ok(t)
}

/// Runs `f` on a plain thread, outside the tokio runtime (see [`build_providers`]).
fn off_runtime<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| s.spawn(f).join().expect("telemetry thread panicked"))
}

/// Installs the global `tracing` subscriber: stdout logs always, plus OTLP traces, metrics and
/// logs for the signals that have an endpoint.
pub fn init() -> anyhow::Result<Telemetry> {
    let t = off_runtime(build_providers)?;

    // The exporters' own HTTP stack must not be exported back through the log/trace layers.
    let not_exporter = filter_fn(|m| {
        let target = m.target();
        !["hyper", "h2", "reqwest", "opentelemetry", "rustls"]
            .iter()
            .any(|p| target.starts_with(p))
    });
    let trace_layer = t.traces.as_ref().map(|p| {
        global::set_tracer_provider(p.clone());
        tracing_opentelemetry::layer()
            .with_tracer(p.tracer("netrack"))
            .with_filter(not_exporter.clone())
    });
    let log_layer = t
        .logs
        .as_ref()
        .map(|p| OpenTelemetryTracingBridge::new(p).with_filter(not_exporter));
    if let Some(p) = &t.metrics {
        global::set_meter_provider(p.clone());
    }
    global::set_text_map_propagator(TraceContextPropagator::new());

    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(tracing_subscriber::fmt::layer())
        .with(trace_layer)
        .with(log_layer)
        .init();
    Ok(t)
}

impl Telemetry {
    /// Flushes and stops every exporter.
    pub fn shutdown(self) {
        off_runtime(move || {
            for r in [
                self.traces.map(|p| p.shutdown()),
                self.metrics.map(|p| p.shutdown()),
                self.logs.map(|p| p.shutdown()),
            ]
            .into_iter()
            .flatten()
            {
                if let Err(e) = r {
                    eprintln!("telemetry shutdown failed: {e}");
                }
            }
        });
    }
}

struct Instruments {
    runs: Counter<u64>,
    degraded: Counter<u64>,
    duration: Histogram<f64>,
    download: Gauge<f64>,
    upload: Gauge<f64>,
    ping: Gauge<f64>,
    jitter: Gauge<f64>,
    http_duration: Histogram<f64>,
}

/// Created on first use, so the meter provider must be installed before (see [`init`]).
fn instruments() -> &'static Instruments {
    static I: OnceLock<Instruments> = OnceLock::new();
    I.get_or_init(|| {
        let m = global::meter("netrack");
        Instruments {
            runs: m
                .u64_counter("netrack.speedtest.runs")
                .with_description("Speed tests run, by result")
                .build(),
            degraded: m
                .u64_counter("netrack.speedtest.degraded")
                .with_description("Tests below the degraded threshold")
                .build(),
            duration: m
                .f64_histogram("netrack.speedtest.duration")
                .with_unit("s")
                .with_description("Time to run one test, retry included")
                .build(),
            download: m
                .f64_gauge("netrack.speedtest.download")
                .with_unit("Mbit/s")
                .with_description("Download speed of the last successful test")
                .build(),
            upload: m
                .f64_gauge("netrack.speedtest.upload")
                .with_unit("Mbit/s")
                .with_description("Upload speed of the last successful test")
                .build(),
            ping: m
                .f64_gauge("netrack.speedtest.ping")
                .with_unit("ms")
                .with_description("Latency of the last successful test")
                .build(),
            jitter: m
                .f64_gauge("netrack.speedtest.jitter")
                .with_unit("ms")
                .with_description("Jitter of the last successful test")
                .build(),
            http_duration: m
                .f64_histogram("http.server.request.duration")
                .with_unit("s")
                .with_description("Duration of HTTP server requests")
                .build(),
        }
    })
}

/// Records one finished test. `metrics` is `None` for a failed one.
pub fn record_run(
    kind: TestKind,
    provider: &str,
    metrics: Option<&Metrics>,
    duration_secs: f64,
    degraded: bool,
) {
    let i = instruments();
    let attrs = [
        KeyValue::new("kind", format!("{kind:?}").to_lowercase()),
        KeyValue::new("provider", provider.to_owned()),
    ];
    let with_result = |r: &'static str| {
        let mut a = attrs.to_vec();
        a.push(KeyValue::new("result", r));
        a
    };
    i.duration.record(duration_secs, &attrs);
    match metrics {
        Some(m) => {
            i.runs.add(1, &with_result("success"));
            i.download.record(m.download, &attrs);
            i.upload.record(m.upload, &attrs);
            i.ping.record(m.ping, &attrs);
            i.jitter.record(m.jitter, &attrs);
            if degraded {
                i.degraded.add(1, &attrs);
            }
        }
        None => i.runs.add(1, &with_result("failure")),
    }
}

struct HeaderExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }
    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Server span and duration metric per request, continuing an incoming `traceparent`.
/// Use as a `route_layer`: it needs the matched route to keep metric cardinality bounded.
pub async fn http_middleware(req: Request, next: Next) -> Response {
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| "unmatched".to_owned(), |p| p.as_str().to_owned());
    if route == "/healthz" {
        return next.run(req).await;
    }
    let method = req.method().to_string();
    let span = tracing::info_span!(
        "http.request",
        otel.name = %format!("{method} {route}"),
        otel.kind = "server",
        http.request.method = %method,
        http.route = %route,
        http.response.status_code = tracing::field::Empty,
    );
    let parent = global::get_text_map_propagator(|p| p.extract(&HeaderExtractor(req.headers())));
    let _ = span.set_parent(parent);

    let started = Instant::now();
    let resp = next.run(req).instrument(span.clone()).await;
    let status = resp.status().as_u16();
    span.record("http.response.status_code", status);
    instruments().http_duration.record(
        started.elapsed().as_secs_f64(),
        &[
            KeyValue::new("http.request.method", method),
            KeyValue::new("http.route", route),
            KeyValue::new("http.response.status_code", i64::from(status)),
        ],
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn off_without_endpoint() {
        let get = env(&[]);
        assert!(
            !["TRACES", "METRICS", "LOGS"]
                .iter()
                .any(|s| signal_enabled(&get, s))
        );
    }

    #[test]
    fn general_endpoint_enables_all_signals() {
        let get = env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318")]);
        assert!(
            ["TRACES", "METRICS", "LOGS"]
                .iter()
                .all(|s| signal_enabled(&get, s))
        );
    }

    #[test]
    fn per_signal_endpoint_enables_only_that_signal() {
        let get = env(&[(
            "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
            "http://c:4318/v1/traces",
        )]);
        assert!(signal_enabled(&get, "TRACES"));
        assert!(!signal_enabled(&get, "METRICS") && !signal_enabled(&get, "LOGS"));
    }

    #[test]
    fn exporter_none_and_sdk_disabled_turn_a_signal_off() {
        let get = env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
            ("OTEL_LOGS_EXPORTER", "none"),
        ]);
        assert!(signal_enabled(&get, "TRACES") && !signal_enabled(&get, "LOGS"));
        let get = env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://c:4318"),
            ("OTEL_SDK_DISABLED", "true"),
        ]);
        assert!(!signal_enabled(&get, "TRACES"));
    }
}
