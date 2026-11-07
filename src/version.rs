use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

pub const DEFAULT_RELEASE_URL: &str =
    "https://api.github.com/repos/Diaszano/netrack/releases/latest";
pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
}

pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches(|c| c == 'v' || c == 'V');
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts
        .next()
        .and_then(|p| {
            let clean = p.split(|c| c == '-' || c == '+').next().unwrap_or(p);
            clean.parse().ok()
        })
        .unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|p| {
            let clean = p.split(|c| c == '-' || c == '+').next().unwrap_or(p);
            clean.parse().ok()
        })
        .unwrap_or(0);
    Some((major, minor, patch))
}

pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

pub fn clean_version_tag(tag: &str) -> String {
    tag.trim().trim_start_matches(|c| c == 'v' || c == 'V').to_string()
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: Option<String>,
    name: Option<String>,
}

#[derive(Clone)]
pub struct VersionChecker {
    pub current_version: String,
    pub release_url: String,
    cache_ttl: Duration,
    cache: Arc<RwLock<Option<(Instant, Option<String>)>>>,
    client: reqwest::Client,
}

impl VersionChecker {
    pub fn new(current_version: impl Into<String>, release_url: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        Self {
            current_version: current_version.into(),
            release_url: release_url.into(),
            cache_ttl: DEFAULT_CACHE_TTL,
            cache: Arc::new(RwLock::new(None)),
            client,
        }
    }

    pub fn with_release_url(mut self, url: impl Into<String>) -> Self {
        self.release_url = url.into();
        self.cache = Arc::new(RwLock::new(None));
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    pub async fn check(&self) -> VersionInfo {
        {
            let cache_read = self.cache.read().await;
            if let Some((cached_at, ref latest)) = *cache_read
                && cached_at.elapsed() < self.cache_ttl
            {
                let update_available = latest
                    .as_deref()
                    .map(|l| is_newer(&self.current_version, l))
                    .unwrap_or(false);
                return VersionInfo {
                    current_version: self.current_version.clone(),
                    latest_version: latest.clone(),
                    update_available,
                };
            }
        }

        // Cache miss or expired: fetch from GitHub
        let latest = match self.fetch_latest_release().await {
            Ok(Some(tag)) => Some(clean_version_tag(&tag)),
            _ => None,
        };

        {
            let mut cache_write = self.cache.write().await;
            *cache_write = Some((Instant::now(), latest.clone()));
        }

        let update_available = latest
            .as_deref()
            .map(|l| is_newer(&self.current_version, l))
            .unwrap_or(false);

        VersionInfo {
            current_version: self.current_version.clone(),
            latest_version: latest,
            update_available,
        }
    }

    async fn fetch_latest_release(&self) -> anyhow::Result<Option<String>> {
        let resp = self
            .client
            .get(&self.release_url)
            .header("User-Agent", format!("netrack/{}", self.current_version))
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let bytes = resp.bytes().await?;
        let release: GithubRelease = serde_json::from_slice(&bytes)?;
        Ok(release.tag_name.or(release.name))
    }
}

impl Default for VersionChecker {
    fn default() -> Self {
        Self::new(env!("CARGO_PKG_VERSION"), DEFAULT_RELEASE_URL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing_and_comparison() {
        assert_eq!(parse_version("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("V2.0.0-beta"), Some((2, 0, 0)));
        assert_eq!(parse_version("3.4"), Some((3, 4, 0)));

        assert!(is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.0", "v0.1.1"));
        assert!(is_newer("0.1.0", "1.0.0"));
        assert!(!is_newer("0.2.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("1.0.0", "0.9.9"));
    }
}
