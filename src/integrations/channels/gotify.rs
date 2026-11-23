use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let server_url = config
        .get("server_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'server_url' in gotify configuration"))?;

    let app_token = config
        .get("app_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'app_token' in gotify configuration"))?;

    let priority = config.get("priority").and_then(|v| v.as_i64()).unwrap_or(5);

    let (title, message) = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => (
            "Speedtest started".to_string(),
            format!("Running a {kind} speedtest"),
        ),
        IntegrationEvent::SpeedtestFinished { metrics, .. } => (
            "Speedtest finished".to_string(),
            format!(
                "↓ {:.2} Mbps / ↑ {:.2} Mbps (ping {:.2} ms)",
                metrics.download, metrics.upload, metrics.ping
            ),
        ),
        IntegrationEvent::SpeedtestDegraded { .. } => (
            "Speedtest degraded".to_string(),
            event.degraded_summary().unwrap_or_default(),
        ),
        IntegrationEvent::SpeedtestRecovered { .. } => (
            "Speedtest recovered".to_string(),
            event.recovered_summary().unwrap_or_default(),
        ),
        IntegrationEvent::SpeedtestFailed { error, .. } => {
            ("Speedtest failed".to_string(), format!("Error: {error}"))
        }
        IntegrationEvent::MinutePassed { .. } => (
            "Heartbeat".to_string(),
            "Netrack instance is healthy".to_string(),
        ),
        IntegrationEvent::ConfigUpdated { .. } => (
            "Configuration updated".to_string(),
            "Settings were modified".to_string(),
        ),
        IntegrationEvent::ExpectedUpdated {
            ping,
            download,
            upload,
            ..
        } => (
            "Expected updated".to_string(),
            format!("ping: {ping:.2}ms, down: {download:.2}Mbps, up: {upload:.2}Mbps"),
        ),
        IntegrationEvent::Test { .. } => (
            "Test notification".to_string(),
            "Gotify notifications from Netrack are working correctly.".to_string(),
        ),
    };

    let url = format!("{}/message", server_url.trim_end_matches('/'));

    let payload = json!({
        "title": format!("Netrack: {title}"),
        "message": message,
        "priority": priority,
    });

    let resp = client
        .post(&url)
        .header("X-Gotify-Key", app_token)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("gotify send failed with status {status}: {body}");
    }

    Ok(())
}
