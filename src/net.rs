//! Network interface discovery and HTTP client construction.

use std::net::IpAddr;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NetInterface {
    pub name: String,
    pub addrs: Vec<IpAddr>,
}

/// Non-loopback interfaces with their addresses, grouped by name.
pub fn list_interfaces() -> Vec<NetInterface> {
    let mut out: Vec<NetInterface> = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if iface.is_loopback() {
            continue;
        }
        let ip = iface.ip();
        match out.iter_mut().find(|i| i.name == iface.name) {
            Some(existing) => existing.addrs.push(ip),
            None => out.push(NetInterface {
                name: iface.name,
                addrs: vec![ip],
            }),
        }
    }
    out
}

/// Picks the local address to bind for `iface` (IPv4 preferred). `None` means
/// "let the OS choose": no interface configured, or the name is unknown.
pub fn resolve_local_addr(iface: Option<&str>) -> Option<IpAddr> {
    let name = iface?;
    let found = list_interfaces().into_iter().find(|i| i.name == name)?;
    found
        .addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(found.addrs.first())
        .copied()
}

/// HTTP/1.1 client bound to `local_addr` when given. HTTP/1.1 only, so every
/// parallel stream is its own TCP connection. Only a connect timeout is set;
/// the throughput engine owns overall timing.
pub fn http_client(local_addr: Option<IpAddr>) -> reqwest::Client {
    reqwest::Client::builder()
        .use_rustls_tls()
        .http1_only()
        .local_address(local_addr)
        .tcp_nodelay(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .expect("static reqwest client configuration is valid")
}
