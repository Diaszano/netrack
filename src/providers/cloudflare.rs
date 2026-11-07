//! Native Cloudflare speed test (`speed.cloudflare.com` endpoints).

use futures::FutureExt;
use futures::future::BoxFuture;

use super::{
    Endpoints, Measurement, ProviderError, RunOptions, SpeedProvider, measure_all, with_run_timeout,
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
        let ep = Endpoints {
            latency_url: Box::new({
                let base = base.clone();
                move || format!("{base}/__down?bytes=0")
            }),
            download_url: format!("{base}/__down?bytes={DOWNLOAD_BYTES}"),
            upload_url: format!("{base}/__up"),
        };
        let (stats, download, upload, headers) = measure_all(&client, opts, ep).await?;
        let colo = headers
            .get("cf-meta-colo")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        Ok(Measurement {
            ping: stats.ping,
            jitter: stats.jitter,
            download,
            upload,
            server_id: None,
            server_name: Some(format!("Cloudflare · {colo}")),
            server_host: Some(HOST.to_string()),
            result_id: None,
        })
    }
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
