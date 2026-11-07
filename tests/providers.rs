mod support;

use std::time::{Duration, Instant};

use netrack::measure::ThroughputConfig;
use netrack::net::resolve_local_addr;
use netrack::providers::librespeed::{
    LibreServer, LibreSource, LibreSpeed, custom_libre_server, fetch_libre_servers,
    pick_libre_server,
};
use netrack::providers::{Cloudflare, ProviderError, RunOptions, SpeedProvider};
use support::spawn_mock;

fn opts() -> RunOptions {
    RunOptions {
        local_addr: None,
        throughput: ThroughputConfig {
            duration: Duration::from_secs(2),
            warmup: Duration::from_millis(500),
            ..ThroughputConfig::default()
        },
    }
}

#[tokio::test]
async fn cloudflare_run_against_mock() {
    let mock = spawn_mock().await;
    let m = Cloudflare::with_base(&mock.base_url)
        .run(&opts())
        .await
        .expect("run");
    assert!(m.download > 0.0 && m.upload > 0.0 && m.ping >= 0.0 && m.jitter >= 0.0);
    // The mock holds each probe for 80 ms of reported server time; ping must
    // exclude it, as speed.cloudflare.com does.
    assert!(m.ping < 40.0, "ping includes server time: {} ms", m.ping);
    assert_eq!(m.server_name.as_deref(), Some("Cloudflare · POA"));
    assert_eq!(m.server_id.as_deref(), Some("POA"));
    assert_eq!(m.server_host.as_deref(), Some("speed.cloudflare.com"));
}

#[tokio::test]
async fn librespeed_run_against_mock() {
    let mock = spawn_mock().await;
    let server = custom_libre_server(&format!("{}/backend/", mock.base_url));
    let m = LibreSpeed::new(LibreSource::Custom(server))
        .run(&opts())
        .await
        .expect("run");
    assert!(m.download > 0.0 && m.upload > 0.0 && m.ping >= 0.0);
    let host = mock.base_url.trim_start_matches("http://");
    assert_eq!(m.server_host.as_deref(), Some(host));
}

fn libre(id: u32, server: &str) -> LibreServer {
    LibreServer {
        id,
        name: format!("server {id}"),
        server: server.to_string(),
        dl_url: "garbage.php".into(),
        ul_url: "empty.php".into(),
        ping_url: "empty.php".into(),
    }
}

#[tokio::test]
async fn pick_libre_server_considers_whole_list() {
    let mock = spawn_mock().await;
    // Eleven unreachable servers first; the only reachable one is last.
    let mut list: Vec<LibreServer> = (1..=11)
        .map(|id| libre(id, "http://127.0.0.1:1/"))
        .collect();
    list.push(libre(12, &format!("{}/backend/", mock.base_url)));
    let picked = pick_libre_server(&reqwest::Client::new(), &list)
        .await
        .expect("reachable server");
    assert_eq!(picked.id, 12);
}

#[test]
fn libre_server_host_is_normalized() {
    assert_eq!(libre(1, "//alpha.example.com/").host(), "alpha.example.com");
    assert_eq!(
        libre(2, "https://beta.example.com/speed/").host(),
        "beta.example.com"
    );
    assert_eq!(
        libre(3, "http://127.0.0.1:8080/backend/").host(),
        "127.0.0.1:8080"
    );
}

#[tokio::test]
async fn librespeed_by_id_and_auto_resolve_from_list() {
    let mock = spawn_mock().await;
    let list_url = format!("{}/servers.json", mock.base_url);
    let missing = LibreSpeed::new(LibreSource::ById {
        list_url: list_url.clone(),
        id: 99,
    });
    assert!(matches!(
        missing.run(&opts()).await,
        Err(ProviderError::NoServer)
    ));
    // Listed servers are unreachable hosts, so a found id fails on the network, not NoServer.
    let found = LibreSpeed::new(LibreSource::ById { list_url, id: 1 });
    let started = Instant::now();
    assert!(matches!(
        found.run(&opts()).await,
        Err(ProviderError::Network(_) | ProviderError::Measure(_))
    ));
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[tokio::test]
async fn fetch_libre_servers_parses_public_shape() {
    let mock = spawn_mock().await;
    let client = reqwest::Client::new();
    let list = fetch_libre_servers(&client, &format!("{}/servers.json", mock.base_url))
        .await
        .expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].id, 1);
    assert_eq!(list[0].dl_url, "garbage.php");
    assert_eq!(list[0].ul_url, "empty.php");
    assert_eq!(list[0].ping_url, "empty.php");
    assert_eq!(list[0].server, "//alpha.example.com/");
}

#[tokio::test]
async fn unreachable_server_is_network_error() {
    let started = Instant::now();
    let res = Cloudflare::with_base("http://127.0.0.1:1")
        .run(&opts())
        .await;
    assert!(matches!(res, Err(ProviderError::Network(_))), "{res:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn resolve_local_addr_auto_is_none() {
    assert_eq!(resolve_local_addr(None), None);
    assert_eq!(resolve_local_addr(Some("definitely-not-an-iface0")), None);
}
