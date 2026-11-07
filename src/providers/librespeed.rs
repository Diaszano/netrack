//! Native LibreSpeed client (public server list or a custom server).

use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, join_all};
use serde::{Deserialize, Serialize};

use super::{
    Endpoints, Measurement, ProviderError, RunOptions, SpeedProvider, measure_all, net_err,
    timed_get, with_run_timeout,
};
use crate::net::http_client;

pub const DEFAULT_LIST_URL: &str = "https://librespeed.org/backend-servers/servers.php";
const PICK_TIMEOUT: Duration = Duration::from_secs(2);
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
const DOWNLOAD_CK_SIZE: u32 = 100;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LibreServer {
    pub id: u32,
    pub name: String,
    pub server: String,
    #[serde(rename = "dlURL")]
    pub dl_url: String,
    #[serde(rename = "ulURL")]
    pub ul_url: String,
    #[serde(rename = "pingURL")]
    pub ping_url: String,
}

impl LibreServer {
    /// `server` as a usable absolute base URL without trailing slash. The
    /// public list uses protocol-relative `//host/` entries.
    fn base(&self) -> String {
        let s = self.server.trim_end_matches('/');
        if s.starts_with("//") {
            format!("https:{s}")
        } else {
            s.to_string()
        }
    }

    /// The server's host (with port when explicit), e.g. `alpha.example.com`.
    /// Falls back to the raw `server` string if it does not parse as a URL.
    pub fn host(&self) -> String {
        let Ok(url) = reqwest::Url::parse(&self.base()) else {
            return self.server.clone();
        };
        match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            (None, _) => self.server.clone(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base(), path.trim_start_matches('/'))
    }
}

/// A user-supplied server URL, using MySpeed's endpoint naming convention.
pub fn custom_libre_server(url: &str) -> LibreServer {
    LibreServer {
        id: 0,
        name: url.to_string(),
        server: url.to_string(),
        dl_url: "garbage.php".into(),
        ul_url: "empty.php".into(),
        ping_url: "empty.php".into(),
    }
}

pub async fn fetch_libre_servers(
    client: &reqwest::Client,
    list_url: &str,
) -> Result<Vec<LibreServer>, ProviderError> {
    let bytes = client
        .get(list_url)
        .timeout(LIST_TIMEOUT)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(net_err)?
        .bytes()
        .await
        .map_err(net_err)?;
    serde_json::from_slice(&bytes).map_err(net_err)
}

/// The lowest-latency reachable server of the whole list, measured with a
/// single ping request each, all probed concurrently.
pub async fn pick_libre_server(
    client: &reqwest::Client,
    servers: &[LibreServer],
) -> Result<LibreServer, ProviderError> {
    let probes = servers.iter().map(|s| async move {
        let started = std::time::Instant::now();
        let url = format!("{}?r={}", s.url(&s.ping_url), rand::random::<u32>());
        timed_get(client, &url, PICK_TIMEOUT)
            .await
            .ok()
            .map(|_| (started.elapsed(), s))
    });
    join_all(probes)
        .await
        .into_iter()
        .flatten()
        .min_by_key(|(rtt, _)| *rtt)
        .map(|(_, s)| s.clone())
        .ok_or(ProviderError::NoServer)
}

pub enum LibreSource {
    Custom(LibreServer),
    ById { list_url: String, id: u32 },
    Auto { list_url: String },
}

pub struct LibreSpeed {
    source: LibreSource,
}

impl LibreSpeed {
    pub fn new(source: LibreSource) -> Self {
        Self { source }
    }

    async fn resolve(&self, client: &reqwest::Client) -> Result<LibreServer, ProviderError> {
        match &self.source {
            LibreSource::Custom(s) => Ok(s.clone()),
            LibreSource::ById { list_url, id } => fetch_libre_servers(client, list_url)
                .await?
                .into_iter()
                .find(|s| s.id == *id)
                .ok_or(ProviderError::NoServer),
            LibreSource::Auto { list_url } => {
                let list = fetch_libre_servers(client, list_url).await?;
                pick_libre_server(client, &list).await
            }
        }
    }

    async fn run_inner(&self, opts: &RunOptions) -> Result<Measurement, ProviderError> {
        let client = http_client(opts.local_addr);
        let server = self.resolve(&client).await?;
        let ep = Endpoints {
            latency_url: Box::new({
                let server = server.clone();
                move || {
                    format!(
                        "{}?r={}",
                        server.url(&server.ping_url),
                        rand::random::<u32>()
                    )
                }
            }),
            download_url: format!("{}?ckSize={DOWNLOAD_CK_SIZE}", server.url(&server.dl_url)),
            upload_url: server.url(&server.ul_url),
        };
        let (stats, download, upload) = measure_all(&client, opts, ep).await?;
        let host = server.host();
        Ok(Measurement {
            ping: stats.ping,
            jitter: stats.jitter,
            download,
            upload,
            server_id: Some(server.id.to_string()),
            server_name: Some(server.name),
            server_host: Some(host),
            result_id: None,
        })
    }
}

impl SpeedProvider for LibreSpeed {
    fn name(&self) -> &'static str {
        "librespeed"
    }

    fn run<'a>(
        &'a self,
        opts: &'a RunOptions,
    ) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        with_run_timeout(self.run_inner(opts)).boxed()
    }
}
