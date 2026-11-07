//! On-demand Ookla CLI provider, downloader, and server listing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::FutureExt;
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

use super::{Measurement, ProviderError, RunOptions, SpeedProvider};

pub const X86_64_URL: &str =
    "https://install.speedtest.net/app/cli/ookla-speedtest-1.2.0-linux-x86_64.tgz";
pub const AARCH64_URL: &str =
    "https://install.speedtest.net/app/cli/ookla-speedtest-1.2.0-linux-aarch64.tgz";

pub fn detect_arch() -> Result<&'static str, &'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("x86_64"),
        "aarch64" => Ok("aarch64"),
        _ => Err("unsupported architecture"),
    }
}

pub fn official_download_url(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some(X86_64_URL),
        "aarch64" => Some(AARCH64_URL),
        _ => None,
    }
}

pub fn unpack_speedtest_tgz(tgz_bytes: &[u8], dest_bin: &Path) -> anyhow::Result<()> {
    let gz = flate2::read::GzDecoder::new(tgz_bytes);
    let mut archive = tar::Archive::new(gz);
    let mut found = false;

    if let Some(parent) = dest_bin.parent() {
        std::fs::create_dir_all(parent)?;
    }

    for entry_result in archive.entries()? {
        let mut entry = entry_result?;
        let path = entry.path()?;
        if path.file_name() == Some(std::ffi::OsStr::new("speedtest")) {
            entry.unpack(dest_bin)?;
            found = true;
            break;
        }
    }

    if !found {
        anyhow::bail!("archive does not contain a 'speedtest' binary");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest_bin, std::fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

pub async fn download_and_install(
    client: &reqwest::Client,
    url: &str,
    dest_bin: &Path,
) -> anyhow::Result<()> {
    let resp = client.get(url).send().await?.error_for_status()?;
    let bytes = resp.bytes().await?;
    unpack_speedtest_tgz(&bytes, dest_bin)?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OoklaServer {
    pub id: u32,
    pub name: String,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RawServerList {
    Wrapped { servers: Vec<RawServer> },
    Array(Vec<RawServer>),
}

#[derive(Deserialize)]
struct RawServer {
    #[serde(deserialize_with = "deserialize_id")]
    id: u32,
    name: String,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    country: Option<String>,
    #[serde(default)]
    host: Option<String>,
}

fn deserialize_id<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(deserializer)?;
    match v {
        serde_json::Value::Number(n) => n
            .as_u64()
            .map(|n| n as u32)
            .ok_or_else(|| serde::de::Error::custom("invalid id number")),
        serde_json::Value::String(s) => s.parse::<u32>().map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("expected integer or string id")),
    }
}

pub fn parse_servers_json(output: &str) -> Result<Vec<OoklaServer>, ProviderError> {
    let json_str = extract_json(output)
        .ok_or_else(|| ProviderError::Binary("no valid JSON found in speedtest output".into()))?;

    let parsed: RawServerList = serde_json::from_str(json_str)
        .map_err(|e| ProviderError::Binary(format!("failed to parse server list JSON: {e}")))?;

    let raw_servers = match parsed {
        RawServerList::Wrapped { servers } => servers,
        RawServerList::Array(servers) => servers,
    };

    Ok(raw_servers
        .into_iter()
        .map(|s| OoklaServer {
            id: s.id,
            name: s.name,
            location: s.location,
            country: s.country,
            host: s.host,
        })
        .collect())
}

pub async fn list_servers(bin_path: &Path) -> Result<Vec<OoklaServer>, ProviderError> {
    let mut cmd = tokio::process::Command::new(bin_path);
    cmd.args(["-L", "--format=json", "--accept-license", "--accept-gdpr"]);
    cmd.kill_on_drop(true);

    let output = tokio::time::timeout(Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| ProviderError::Network("timeout listing servers".into()))?
        .map_err(|e| ProviderError::Binary(format!("failed to execute speedtest: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ProviderError::Binary(format!(
            "speedtest -L failed: {stderr}"
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_servers_json(&stdout)
}

#[derive(Debug, Clone)]
pub struct Ookla {
    bin_path: PathBuf,
    server_id: Option<u32>,
}

impl Ookla {
    pub fn new(bin_path: PathBuf, server_id: Option<u32>) -> Self {
        Self {
            bin_path,
            server_id,
        }
    }

    async fn run_inner(&self, opts: &RunOptions) -> Result<Measurement, ProviderError> {
        let mut args = vec![
            "--format=json-pretty".to_string(),
            "--accept-license".to_string(),
            "--accept-gdpr".to_string(),
        ];
        if let Some(id) = self.server_id {
            args.push("--server-id".to_string());
            args.push(id.to_string());
        }
        if let Some(ip) = opts.local_addr {
            args.push("--ip".to_string());
            args.push(ip.to_string());
        }

        let mut cmd = tokio::process::Command::new(&self.bin_path);
        cmd.args(&args);
        cmd.kill_on_drop(true);

        let output = tokio::time::timeout(Duration::from_secs(60), cmd.output())
            .await
            .map_err(|_| ProviderError::Network("speedtest execution timeout (60s)".into()))?
            .map_err(|e| ProviderError::Binary(format!("failed to execute speedtest: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Binary(format!(
                "speedtest failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_measurement_json(&stdout)
    }
}

impl SpeedProvider for Ookla {
    fn name(&self) -> &'static str {
        "ookla"
    }

    fn run<'a>(
        &'a self,
        opts: &'a RunOptions,
    ) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move { self.run_inner(opts).await }.boxed()
    }
}

#[derive(Deserialize)]
struct OoklaRawOutput {
    ping: Option<OoklaRawPing>,
    download: Option<OoklaRawBandwidth>,
    upload: Option<OoklaRawBandwidth>,
    server: Option<OoklaRawServer>,
    result: Option<OoklaRawResult>,
}

#[derive(Deserialize)]
struct OoklaRawPing {
    jitter: Option<f64>,
    latency: Option<f64>,
}

#[derive(Deserialize)]
struct OoklaRawBandwidth {
    bandwidth: Option<u64>,
}

#[derive(Deserialize)]
struct OoklaRawServer {
    #[serde(default, deserialize_with = "deserialize_optional_id")]
    id: Option<String>,
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

#[derive(Deserialize)]
struct OoklaRawResult {
    id: Option<String>,
    url: Option<String>,
}

fn deserialize_optional_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Option::<serde_json::Value>::deserialize(deserializer)?;
    match v {
        Some(serde_json::Value::Number(n)) => Ok(Some(n.to_string())),
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        _ => Ok(None),
    }
}

fn extract_json(output: &str) -> Option<&str> {
    let trimmed = output.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return Some(trimmed);
    }
    let first = output.find(['{', '['])?;
    let last = output.rfind(['}', ']'])?;
    if first < last {
        Some(&output[first..=last])
    } else {
        None
    }
}

pub fn parse_measurement_json(output: &str) -> Result<Measurement, ProviderError> {
    let json_str = extract_json(output)
        .ok_or_else(|| ProviderError::Binary("no valid JSON found in speedtest output".into()))?;

    let raw: OoklaRawOutput = serde_json::from_str(json_str)
        .map_err(|e| ProviderError::Binary(format!("failed to parse speedtest JSON: {e}")))?;

    let ping = raw.ping.as_ref().and_then(|p| p.latency).unwrap_or(0.0);
    let jitter = raw.ping.as_ref().and_then(|p| p.jitter).unwrap_or(0.0);
    // bytes/sec to Mbps: bytes/sec * 8 / 1_000_000
    let download = raw
        .download
        .as_ref()
        .and_then(|d| d.bandwidth)
        .map(|b| (b as f64) * 8.0 / 1_000_000.0)
        .unwrap_or(0.0);
    let upload = raw
        .upload
        .as_ref()
        .and_then(|u| u.bandwidth)
        .map(|b| (b as f64) * 8.0 / 1_000_000.0)
        .unwrap_or(0.0);

    let server_id = raw.server.as_ref().and_then(|s| s.id.clone());
    let server_name = raw.server.as_ref().and_then(|s| s.name.clone());
    let server_host = raw.server.as_ref().and_then(|s| match (&s.host, s.port) {
        (Some(host), Some(port)) if !host.contains(':') => Some(format!("{host}:{port}")),
        (Some(host), _) => Some(host.clone()),
        _ => None,
    });
    let result_id = raw
        .result
        .as_ref()
        .and_then(|r| r.id.clone().or_else(|| r.url.clone()));

    Ok(Measurement {
        ping,
        jitter,
        download,
        upload,
        server_id,
        server_name,
        server_host,
        result_id,
    })
}
