use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let webhook_url = config
        .get("webhook_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'webhook_url' in discord configuration"))?;

    let username = config
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("Netrack");

    let avatar_url = config.get("avatar_url").and_then(|v| v.as_str());

    let (title, description, color, fields) = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => (
            "Speedtest started".to_string(),
            format!("Running a {kind} speedtest..."),
            0x8B7CFF, // Purple
            vec![],
        ),
        IntegrationEvent::SpeedtestFinished {
            metrics,
            server_name,
            provider,
            ..
        } => {
            let mut f = vec![
                json!({ "name": "Download", "value": format!("{:.2} Mbps", metrics.download), "inline": true }),
                json!({ "name": "Upload", "value": format!("{:.2} Mbps", metrics.upload), "inline": true }),
                json!({ "name": "Ping", "value": format!("{:.2} ms", metrics.ping), "inline": true }),
                json!({ "name": "Jitter", "value": format!("{:.2} ms", metrics.jitter), "inline": true }),
            ];
            if let Some(srv) = server_name {
                f.push(json!({ "name": "Server", "value": srv, "inline": true }));
            }
            if let Some(prov) = provider {
                f.push(json!({ "name": "Provider", "value": prov, "inline": true }));
            }
            (
                "Speedtest finished".to_string(),
                format!(
                    "↓ {:.2} Mbps | ↑ {:.2} Mbps | ⏱ {:.2} ms",
                    metrics.download, metrics.upload, metrics.ping
                ),
                0x3FD6C2, // Teal
                f,
            )
        }
        IntegrationEvent::SpeedtestFailed { error, .. } => (
            "Speedtest failed".to_string(),
            format!("Error: {error}"),
            0xFF453A, // Red
            vec![],
        ),
        IntegrationEvent::MinutePassed { .. } => (
            "Netrack Heartbeat".to_string(),
            "Netrack is running normally.".to_string(),
            0x8A8DB0,
            vec![],
        ),
        IntegrationEvent::ConfigUpdated { .. } => (
            "Configuration updated".to_string(),
            "Netrack configuration was updated.".to_string(),
            0x8B7CFF,
            vec![],
        ),
        IntegrationEvent::ExpectedUpdated {
            ping,
            download,
            upload,
            ..
        } => (
            "Expected values updated".to_string(),
            format!("Ping: {ping:.2} ms | Download: {download:.2} Mbps | Upload: {upload:.2} Mbps"),
            0x8B7CFF,
            vec![
                json!({ "name": "Ping", "value": format!("{ping:.2} ms"), "inline": true }),
                json!({ "name": "Download", "value": format!("{download:.2} Mbps"), "inline": true }),
                json!({ "name": "Upload", "value": format!("{upload:.2} Mbps"), "inline": true }),
            ],
        ),
        IntegrationEvent::Test { .. } => (
            "Netrack Test Notification".to_string(),
            "This is a test notification from Netrack. Everything is configured correctly!"
                .to_string(),
            0x3FD6C2,
            vec![],
        ),
    };

    let mut embed = json!({
        "title": title,
        "description": description,
        "color": color,
        "fields": fields,
        "footer": { "text": "Netrack" }
    });

    if let Some(obj) = embed.as_object_mut() {
        obj.insert(
            "timestamp".to_string(),
            json!(chrono::Utc::now().to_rfc3339()),
        );
    }

    let mut payload = json!({
        "username": username,
        "embeds": [embed]
    });

    if let Some(av) = avatar_url {
        payload["avatar_url"] = json!(av);
    }

    let resp = client
        .post(webhook_url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("discord webhook failed with status {status}: {body}");
    }

    Ok(())
}
