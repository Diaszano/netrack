use serde_json::{Value, json};

use crate::integrations::IntegrationEvent;

pub async fn send(
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let url = config
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing 'url' in webhook configuration"))?;

    let secret = config.get("secret").and_then(|v| v.as_str());

    let payload = json!({
        "event": event.event_name(),
        "timestamp": event.timestamp(),
        "data": event.to_json_data(),
    });

    let mut req = client
        .post(url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&payload)?);

    if let Some(s) = secret {
        req = req.header("X-Webhook-Secret", s);
    }

    if let Some(headers_obj) = config.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers_obj {
            if let Some(str_val) = v.as_str() {
                req = req.header(k, str_val);
            }
        }
    }

    let resp = req.send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("webhook failed with status {status}: {body}");
    }

    Ok(())
}
