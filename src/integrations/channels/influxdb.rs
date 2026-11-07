use serde_json::Value;

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let url = config
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'url' in influxdb configuration"))?;

    let version = config
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("v2");

    let measurement = config
        .get("measurement")
        .and_then(|v| v.as_str())
        .unwrap_or("netrack");

    let timestamp_s = event.timestamp() / 1000;

    let line = match event {
        IntegrationEvent::SpeedtestFinished {
            metrics,
            server_name,
            provider,
            ..
        } => {
            let mut tags = "host=netrack".to_string();
            if let Some(p) = provider {
                tags.push_str(&format!(",provider={}", p.replace([' ', ','], "_")));
            }
            if let Some(s) = server_name {
                tags.push_str(&format!(",server={}", s.replace([' ', ','], "_")));
            }
            format!(
                "{measurement},{tags} download={:.2},upload={:.2},ping={:.2},jitter={:.2} {timestamp_s}",
                metrics.download, metrics.upload, metrics.ping, metrics.jitter
            )
        }
        _ => {
            format!(
                "{measurement},host=netrack,event={} value=1 {timestamp_s}",
                event.event_name()
            )
        }
    };

    let target_url = if version == "v1" {
        let db = config
            .get("bucket")
            .or_else(|| config.get("database"))
            .and_then(|v| v.as_str())
            .unwrap_or("netrack");
        format!("{}/write?db={db}&precision=s", url.trim_end_matches('/'))
    } else {
        let org = config
            .get("org")
            .and_then(|v| v.as_str())
            .unwrap_or("netrack");
        let bucket = config
            .get("bucket")
            .and_then(|v| v.as_str())
            .unwrap_or("netrack");
        format!(
            "{}/api/v2/write?org={org}&bucket={bucket}&precision=s",
            url.trim_end_matches('/')
        )
    };

    let mut req = client
        .post(&target_url)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(line);

    if let Some(token) = config.get("token").and_then(|v| v.as_str()) {
        req = req.header("Authorization", format!("Token {token}"));
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("influxdb write failed with status {status}: {body}");
    }

    Ok(())
}
