mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use futures::FutureExt;
use netrack::measure::{
    MeasureError, StreamFactory, ThroughputConfig, drain_counting, latency_stats,
    measure_throughput, upload_body,
};
use support::{MockOptions, spawn_mock, spawn_mock_with};

/// Bytes requested per transfer; the engine re-invokes the factory, so this
/// only needs to be large enough to keep a connection busy for a while.
const TRANSFER_BYTES: u64 = 256 * 1024 * 1024;

fn short_cfg() -> ThroughputConfig {
    ThroughputConfig {
        duration: Duration::from_secs(2),
        warmup: Duration::from_millis(500),
        ..ThroughputConfig::default()
    }
}

fn download_factory(base_url: &str) -> StreamFactory {
    let client = reqwest::Client::new();
    let url = format!("{base_url}/down?bytes={TRANSFER_BYTES}");
    Arc::new(move |counter: Arc<AtomicU64>| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| MeasureError::Http(e.to_string()))?;
            drain_counting(resp, &counter).await
        }
        .boxed()
    })
}

fn upload_factory(base_url: &str) -> StreamFactory {
    let client = reqwest::Client::new();
    let url = format!("{base_url}/up");
    Arc::new(move |counter: Arc<AtomicU64>| {
        let client = client.clone();
        let url = url.clone();
        async move {
            let resp = client
                .post(&url)
                .body(upload_body(TRANSFER_BYTES, counter))
                .send()
                .await
                .map_err(|e| MeasureError::Http(e.to_string()))?;
            resp.error_for_status()
                .map_err(|e| MeasureError::Http(e.to_string()))?;
            Ok(())
        }
        .boxed()
    })
}

#[test]
fn latency_stats_median_and_jitter() {
    let stats = latency_stats(&[10.0, 12.0, 11.0, 30.0, 12.0]).expect("enough samples");
    assert_eq!(stats.ping, 12.0);
    assert_eq!(stats.jitter, 10.0);
    assert!(latency_stats(&[5.0]).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn download_against_mock_reports_positive_rate() {
    let mock = spawn_mock().await;
    let mbps = measure_throughput(&short_cfg(), download_factory(&mock.base_url))
        .await
        .expect("download measurement");
    assert!(mbps > 100.0, "download rate too low: {mbps} Mbps");
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_against_mock_reports_positive_rate() {
    let mock = spawn_mock().await;
    let mbps = measure_throughput(&short_cfg(), upload_factory(&mock.base_url))
        .await
        .expect("upload measurement");
    assert!(mbps > 100.0, "upload rate too low: {mbps} Mbps");
}

#[tokio::test(flavor = "multi_thread")]
async fn download_times_out_on_stalled_server() {
    let mock = spawn_mock_with(MockOptions {
        stall_after_bytes: Some(1_000_000),
        ..MockOptions::default()
    })
    .await;
    let cfg = ThroughputConfig {
        stall_timeout: Duration::from_secs(1),
        ..ThroughputConfig::default()
    };
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(4),
        measure_throughput(&cfg, download_factory(&mock.base_url)),
    )
    .await
    .expect("stall must be detected within 4s");
    assert!(
        matches!(result, Err(MeasureError::Stalled)),
        "expected Stalled, got {result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(4));
}

#[tokio::test(flavor = "multi_thread")]
async fn all_streams_failing_is_error() {
    let mock = spawn_mock_with(MockOptions {
        fail_status: Some(500),
        ..MockOptions::default()
    })
    .await;
    let result = measure_throughput(&short_cfg(), download_factory(&mock.base_url)).await;
    match result {
        Err(MeasureError::AllStreamsFailed(msg)) => {
            assert!(
                msg.contains("500"),
                "last error should mention the status: {msg}"
            )
        }
        other => panic!("expected AllStreamsFailed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_responses_are_failures_and_back_off() {
    let mock = spawn_mock().await;
    let client = reqwest::Client::new();
    let url = format!("{}/empty", mock.base_url);
    let requests = Arc::new(AtomicUsize::new(0));
    let factory: StreamFactory = {
        let requests = requests.clone();
        Arc::new(move |counter: Arc<AtomicU64>| {
            let (client, url, requests) = (client.clone(), url.clone(), requests.clone());
            async move {
                requests.fetch_add(1, Ordering::SeqCst);
                let resp = client
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| MeasureError::Http(e.to_string()))?;
                drain_counting(resp, &counter).await
            }
            .boxed()
        })
    };
    let result = measure_throughput(&short_cfg(), factory).await;
    assert!(
        matches!(result, Err(MeasureError::AllStreamsFailed(_))),
        "expected AllStreamsFailed, got {result:?}"
    );
    // 4 streams retrying every 100 ms for 2 s is ~80 requests.
    let n = requests.load(Ordering::SeqCst);
    assert!(n < 200, "empty responses hammered the server: {n} requests");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "release-mode loopback benchmark: cargo test --release --test measure -- --ignored loopback"]
async fn loopback_exceeds_2_5_gbps() {
    let mock = spawn_mock().await;
    let mbps = measure_throughput(
        &ThroughputConfig::default(),
        download_factory(&mock.base_url),
    )
    .await
    .expect("loopback measurement");
    println!("loopback download: {mbps} Mbps");
    assert!(mbps > 2500.0, "loopback rate too low: {mbps} Mbps");
}
