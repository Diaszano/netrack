use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let user_key = config
        .get("user_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'user_key' in pushover configuration"))?;

    let api_token = config
        .get("api_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'api_token' in pushover configuration"))?;

    let base_url = config
        .get("api_url")
        .or_else(|| config.get("server_url"))
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.pushover.net");

    let device = config.get("device").and_then(|v| v.as_str());
    let priority = config.get("priority").and_then(|v| v.as_i64());

    let (title, message) = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => (
            "Speedtest started".to_string(),
            format!("Running {kind} speedtest"),
        ),
        IntegrationEvent::SpeedtestFinished { metrics, .. } => (
            "Speedtest finished".to_string(),
            format!(
                "↓ {:.2} Mbps / ↑ {:.2} Mbps (ping {:.2} ms)",
                metrics.download, metrics.upload, metrics.ping
            ),
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
            "Settings were updated".to_string(),
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
            "Test Notification".to_string(),
            "Pushover notifications are working!".to_string(),
        ),
    };

    let url = format!("{}/1/messages.json", base_url.trim_end_matches('/'));

    let mut payload = json!({
        "token": api_token,
        "user": user_key,
        "title": format!("Netrack: {title}"),
        "message": message,
    });

    if let Some(d) = device {
        payload["device"] = json!(d);
    }
    if let Some(p) = priority {
        payload["priority"] = json!(p);
    }

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("pushover send failed with status {status}: {body}");
    }

    Ok(())
}
