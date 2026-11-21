use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let bot_token = config
        .get("bot_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'bot_token' in telegram configuration"))?;

    let chat_id = config
        .get("chat_id")
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
        })
        .ok_or_else(|| anyhow::anyhow!("missing 'chat_id' in telegram configuration"))?;

    let base_url = config
        .get("api_url")
        .and_then(|v| v.as_str())
        .unwrap_or("https://api.telegram.org");

    let text = match event {
        IntegrationEvent::SpeedtestStarted { kind, .. } => {
            format!("🚀 *Netrack: Speedtest started*\nRunning a _{kind}_ speedtest...")
        }
        IntegrationEvent::SpeedtestFinished {
            metrics,
            server_name,
            provider,
            ..
        } => {
            let mut msg = format!(
                "✅ *Netrack: Speedtest finished*\n\n\
                 *Download:* {:.2} Mbps\n\
                 *Upload:* {:.2} Mbps\n\
                 *Ping:* {:.2} ms\n\
                 *Jitter:* {:.2} ms",
                metrics.download, metrics.upload, metrics.ping, metrics.jitter
            );
            if let Some(srv) = server_name {
                msg.push_str(&format!("\n*Server:* {srv}"));
            }
            if let Some(prov) = provider {
                msg.push_str(&format!("\n*Provider:* {prov}"));
            }
            msg
        }
        IntegrationEvent::SpeedtestDegraded { .. } => {
            format!(
                "🐌 *Netrack: Speedtest degraded*\n\n{}",
                event.degraded_summary().unwrap_or_default()
            )
        }
        IntegrationEvent::SpeedtestFailed { error, .. } => {
            format!("❌ *Netrack: Speedtest failed*\n\nError: `{error}`")
        }
        IntegrationEvent::MinutePassed { .. } => {
            "💓 *Netrack Heartbeat*\nNetrack is running normally.".to_string()
        }
        IntegrationEvent::ConfigUpdated { .. } => {
            "⚙️ *Netrack: Configuration updated*\nConfiguration was modified.".to_string()
        }
        IntegrationEvent::ExpectedUpdated {
            ping,
            download,
            upload,
            ..
        } => {
            format!(
                "🎯 *Netrack: Expected values updated*\n\
                 *Ping:* {ping:.2} ms\n\
                 *Download:* {download:.2} Mbps\n\
                 *Upload:* {upload:.2} Mbps"
            )
        }
        IntegrationEvent::Test { .. } => {
            "🎉 *Netrack: Test notification*\nTelegram integration is working correctly!"
                .to_string()
        }
    };

    let url = format!(
        "{}/bot{}/sendMessage",
        base_url.trim_end_matches('/'),
        bot_token
    );

    let payload = json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "Markdown"
    });

    let resp = client
        .post(&url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("telegram send failed with status {status}: {body}");
    }

    Ok(())
}
