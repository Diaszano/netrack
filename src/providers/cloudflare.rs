//! Native Cloudflare speed test (`speed.cloudflare.com` endpoints).

use futures::FutureExt;
use futures::future::BoxFuture;

use super::{
    Endpoints, Measurement, PROBE_TIMEOUT, ProviderError, RunOptions, SpeedProvider, measure_all,
    with_run_timeout,
};
use crate::net::http_client;

const DEFAULT_BASE: &str = "https://speed.cloudflare.com";
const HOST: &str = "speed.cloudflare.com";
const DOWNLOAD_BYTES: u64 = 25_000_000;

pub struct Cloudflare {
    base_url: String,
}

impl Cloudflare {
    pub fn new() -> Self {
        Self::with_base(DEFAULT_BASE)
    }

    pub fn with_base(url: &str) -> Self {
        Self {
            base_url: url.trim_end_matches('/').to_string(),
        }
    }

    async fn run_inner(&self, opts: &RunOptions) -> Result<Measurement, ProviderError> {
        let client = http_client(opts.local_addr);
        let base = self.base_url.clone();
        let colo = fetch_colo(&client, &base).await;
        let ep = Endpoints {
            latency_url: Box::new({
                let base = base.clone();
                move || format!("{base}/__down?bytes=0")
            }),
            download_url: format!("{base}/__down?bytes={DOWNLOAD_BYTES}"),
            upload_url: format!("{base}/__up"),
        };
        let (stats, download, upload) = measure_all(&client, opts, ep).await?;
        let name = format!("Cloudflare · {}", colo.as_deref().unwrap_or("unknown"));
        Ok(Measurement {
            ping: stats.ping,
            jitter: stats.jitter,
            download,
            upload,
            server_id: colo,
            server_name: Some(name),
            server_host: Some(HOST.to_string()),
            result_id: None,
        })
    }
}

/// The Cloudflare data center serving us (e.g. `GRU`), from `/cdn-cgi/trace`.
/// `None` if the request fails; the server name is informational only.
async fn fetch_colo(client: &reqwest::Client, base: &str) -> Option<String> {
    let resp = client
        .get(format!("{base}/cdn-cgi/trace"))
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .ok()?;
    trace_colo(&resp.text().await.ok()?)
}

/// The `colo=` value of a `/cdn-cgi/trace` body (`key=value` lines).
fn trace_colo(body: &str) -> Option<String> {
    body.lines()
        .find_map(|line| line.trim().strip_prefix("colo="))
        .map(str::trim)
        .filter(|colo| !colo.is_empty())
        .map(str::to_owned)
}

impl Default for Cloudflare {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedProvider for Cloudflare {
    fn name(&self) -> &'static str {
        "cloudflare"
    }

    fn run<'a>(
        &'a self,
        opts: &'a RunOptions,
    ) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        with_run_timeout(self.run_inner(opts)).boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_colo_parsed() {
        assert_eq!(trace_colo("fl=1\nh=x\ncolo=POA\n"), Some("POA".into()));
        assert_eq!(trace_colo("colo= GRU \r\nloc=BR"), Some("GRU".into()));
        assert_eq!(trace_colo("fl=1\nh=x\n"), None);
        assert_eq!(trace_colo("colo=\n"), None);
    }
}
