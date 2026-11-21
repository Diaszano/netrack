use serde_json::Value;

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let ping_url = config
        .get("ping_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'ping_url' in healthchecks configuration"))?;

    let base = ping_url.trim_end_matches('/');

    let (url, body) = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => {
            (format!("{base}/start"), format!("Started {kind} speedtest"))
        }
        IntegrationEvent::SpeedtestDegraded { .. } => (
            format!("{base}/fail"),
            format!(
                "Speedtest degraded: {}",
                event.degraded_summary().unwrap_or_default()
            ),
        ),
        IntegrationEvent::SpeedtestFailed { error, .. } => {
            (format!("{base}/fail"), format!("Speedtest failed: {error}"))
        }
        IntegrationEvent::SpeedtestFinished { metrics, .. } => (
            base.to_string(),
            format!(
                "Speedtest finished: download={:.2} Mbps, upload={:.2} Mbps, ping={:.2} ms",
                metrics.download, metrics.upload, metrics.ping
            ),
        ),
        IntegrationEvent::MinutePassed { .. } => {
            (base.to_string(), "Minute passed heartbeat".to_string())
        }
        IntegrationEvent::ConfigUpdated { .. } => (base.to_string(), "Config updated".to_string()),
        IntegrationEvent::ExpectedUpdated {
            ping,
            download,
            upload,
            ..
        } => (
            base.to_string(),
            format!("Expected updated: ping={ping:.2}, down={download:.2}, up={upload:.2}"),
        ),
        IntegrationEvent::Test { .. } => (base.to_string(), "Netrack test ping".to_string()),
    };

    let resp = client
        .post(&url)
        .header("Content-Type", "text/plain")
        .body(body)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("healthchecks ping failed with status {status}: {body}");
    }

    Ok(())
}
