pub mod discord;
pub mod gotify;
pub mod healthchecks;
pub mod influxdb;
pub mod ntfy;
pub mod pushover;
pub mod telegram;
pub mod webhook;

use serde_json::Value;

use crate::integrations::IntegrationEvent;

pub async fn dispatch(
    channel_type: &str,
    client: &reqwest::Client,
    config: &Value,
    event: &IntegrationEvent,
) -> anyhow::Result<()> {
    let sent = match channel_type {
        "discord" => discord::send(client, config, event).await,
        "telegram" => telegram::send(client, config, event).await,
        "pushover" => pushover::send(client, config, event).await,
        "gotify" => gotify::send(client, config, event).await,
        "ntfy" => ntfy::send(client, config, event).await,
        "webhook" => webhook::send(client, config, event).await,
        "influxdb" => influxdb::send(client, config, event).await,
        "healthchecks" => healthchecks::send(client, config, event).await,
        other => anyhow::bail!("unsupported channel_type: '{other}'"),
    };
    // reqwest errors print the request URL, which carries secrets (telegram token, webhook urls).
    sent.map_err(|e| match e.downcast::<reqwest::Error>() {
        Ok(re) => re.without_url().into(),
        Err(e) => e,
    })
}
