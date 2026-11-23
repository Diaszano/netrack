use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let topic = config
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'topic' in ntfy configuration"))?;

    let server_url = config
        .get("server_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://ntfy.sh");

    let auth_token = config.get("auth_token").and_then(|v| v.as_str());

    let (title, message, tags) = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => (
            "Speedtest started".to_string(),
            format!("Running a {kind} speedtest"),
            vec!["hourglass"],
        ),
        IntegrationEvent::SpeedtestFinished { metrics, .. } => (
            "Speedtest finished".to_string(),
            format!(
                "↓ {:.2} Mbps / ↑ {:.2} Mbps (ping {:.2} ms)",
                metrics.download, metrics.upload, metrics.ping
            ),
            vec!["rocket", "white_check_mark"],
        ),
        IntegrationEvent::SpeedtestDegraded { .. } => (
            "Speedtest degraded".to_string(),
            event.degraded_summary().unwrap_or_default(),
            vec!["warning", "snail"],
        ),
        IntegrationEvent::SpeedtestRecovered { .. } => (
            "Speedtest recovered".to_string(),
            event.recovered_summary().unwrap_or_default(),
            vec!["white_check_mark"],
        ),
        IntegrationEvent::SpeedtestFailed { error, .. } => (
            "Speedtest failed".to_string(),
            format!("Error: {error}"),
            vec!["x", "warning"],
        ),
        IntegrationEvent::MinutePassed { .. } => (
            "Heartbeat".to_string(),
            "Netrack heartbeat ping".to_string(),
            vec!["heart"],
        ),
        IntegrationEvent::ConfigUpdated { .. } => (
            "Configuration updated".to_string(),
            "Netrack settings changed".to_string(),
            vec!["gear"],
        ),
        IntegrationEvent::ExpectedUpdated {
            ping,
            download,
            upload,
            ..
        } => (
            "Expected values updated".to_string(),
            format!("ping: {ping:.2}ms, down: {download:.2}Mbps, up: {upload:.2}Mbps"),
            vec!["chart_with_upwards_trend"],
        ),
        IntegrationEvent::Test { .. } => (
            "Test notification".to_string(),
            "Netrack ntfy notification is working correctly!".to_string(),
            vec!["tada"],
        ),
    };

    let url = format!("{}/{}", server_url.trim_end_matches('/'), topic);

    let payload = json!({
        "topic": topic,
        "title": format!("Netrack: {title}"),
        "message": message,
        "tags": tags,
    });

    let mut req = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?);

    if let Some(token) = auth_token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("ntfy send failed with status {status}: {body}");
    }

    Ok(())
}
