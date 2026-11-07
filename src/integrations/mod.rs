pub mod channels;
pub mod routes;

use std::time::Duration;

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};

use crate::db::Db;
use crate::db::tests_repo::{Metrics, TestKind};
use crate::runner::RunEvent;

pub const SUPPORTED_CHANNELS: &[&str] = &[
    "discord",
    "telegram",
    "pushover",
    "gotify",
    "ntfy",
    "webhook",
    "influxdb",
    "healthchecks",
];

pub const SUPPORTED_EVENTS: &[&str] = &[
    "speedtest.started",
    "speedtest.finished",
    "speedtest.failed",
    "minute.passed",
    "config.updated",
    "expected.updated",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Integration {
    pub id: i64,
    pub name: String,
    pub channel_type: String,
    pub config: Value,
    pub events: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub enum IntegrationEvent {
    SpeedtestStarted {
        kind: String,
        timestamp: i64,
    },
    SpeedtestFinished {
        id: i64,
        metrics: Metrics,
        server_name: Option<String>,
        server_id: Option<String>,
        server_host: Option<String>,
        provider: Option<String>,
        timestamp: i64,
    },
    SpeedtestFailed {
        id: Option<i64>,
        error: String,
        timestamp: i64,
    },
    MinutePassed {
        timestamp: i64,
    },
    ConfigUpdated {
        timestamp: i64,
    },
    ExpectedUpdated {
        ping: f64,
        download: f64,
        upload: f64,
        timestamp: i64,
    },
    Test {
        timestamp: i64,
    },
}

impl IntegrationEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            IntegrationEvent::SpeedtestStarted { .. } => "speedtest.started",
            IntegrationEvent::SpeedtestFinished { .. } => "speedtest.finished",
            IntegrationEvent::SpeedtestFailed { .. } => "speedtest.failed",
            IntegrationEvent::MinutePassed { .. } => "minute.passed",
            IntegrationEvent::ConfigUpdated { .. } => "config.updated",
            IntegrationEvent::ExpectedUpdated { .. } => "expected.updated",
            IntegrationEvent::Test { .. } => "test",
        }
    }

    pub fn timestamp(&self) -> i64 {
        match self {
            IntegrationEvent::SpeedtestStarted { timestamp, .. } => *timestamp,
            IntegrationEvent::SpeedtestFinished { timestamp, .. } => *timestamp,
            IntegrationEvent::SpeedtestFailed { timestamp, .. } => *timestamp,
            IntegrationEvent::MinutePassed { timestamp, .. } => *timestamp,
            IntegrationEvent::ConfigUpdated { timestamp, .. } => *timestamp,
            IntegrationEvent::ExpectedUpdated { timestamp, .. } => *timestamp,
            IntegrationEvent::Test { timestamp, .. } => *timestamp,
        }
    }

    pub fn to_json_data(&self) -> Value {
        match self {
            IntegrationEvent::SpeedtestStarted { kind, .. } => {
                json!({ "kind": kind })
            }
            IntegrationEvent::SpeedtestFinished {
                id,
                metrics,
                server_name,
                server_id,
                server_host,
                provider,
                ..
            } => {
                json!({
                    "id": id,
                    "metrics": {
                        "download": metrics.download,
                        "upload": metrics.upload,
                        "ping": metrics.ping,
                        "jitter": metrics.jitter,
                    },
                    "server": {
                        "name": server_name,
                        "id": server_id,
                        "host": server_host,
                    },
                    "provider": provider,
                })
            }
            IntegrationEvent::SpeedtestFailed { id, error, .. } => {
                json!({
                    "id": id,
                    "error": error,
                })
            }
            IntegrationEvent::MinutePassed { .. } => {
                json!({ "status": "heartbeat" })
            }
            IntegrationEvent::ConfigUpdated { .. } => {
                json!({ "status": "updated" })
            }
            IntegrationEvent::ExpectedUpdated {
                ping,
                download,
                upload,
                ..
            } => {
                json!({
                    "expected": {
                        "ping": ping,
                        "download": download,
                        "upload": upload,
                    }
                })
            }
            IntegrationEvent::Test { .. } => {
                json!({ "message": "Netrack test notification" })
            }
        }
    }
}

const INTEGRATION_COLUMNS: &str = "id, name, channel_type, config_json, events_json, enabled";

fn map_integration(r: &Row) -> rusqlite::Result<Integration> {
    let config_json: String = r.get(3)?;
    let events_json: String = r.get(4)?;
    let enabled: i64 = r.get(5)?;
    Ok(Integration {
        id: r.get(0)?,
        name: r.get(1)?,
        channel_type: r.get(2)?,
        config: serde_json::from_str(&config_json).unwrap_or(Value::Null),
        events: serde_json::from_str(&events_json).unwrap_or_default(),
        enabled: enabled != 0,
    })
}

pub async fn list(db: &Db) -> anyhow::Result<Vec<Integration>> {
    db.call(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT {INTEGRATION_COLUMNS} FROM integrations ORDER BY id ASC"
        ))?;
        let rows = stmt.query_map([], map_integration)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> anyhow::Result<Option<Integration>> {
    db.call(move |conn| {
        Ok(conn
            .query_row(
                &format!("SELECT {INTEGRATION_COLUMNS} FROM integrations WHERE id = ?1"),
                params![id],
                map_integration,
            )
            .optional()?)
    })
    .await
}

pub async fn insert(
    db: &Db,
    name: String,
    channel_type: String,
    config: Value,
    events: Vec<String>,
    enabled: bool,
) -> anyhow::Result<Integration> {
    let config_json = serde_json::to_string(&config)?;
    let events_json = serde_json::to_string(&events)?;
    let enabled_int = if enabled { 1 } else { 0 };

    let n = name.clone();
    let ct = channel_type.clone();
    let id = db
        .call(move |conn| {
            conn.execute(
                "INSERT INTO integrations (name, channel_type, config_json, events_json, enabled) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![n, ct, config_json, events_json, enabled_int],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .await?;

    Ok(Integration {
        id,
        name,
        channel_type,
        config,
        events,
        enabled,
    })
}

pub async fn update(
    db: &Db,
    id: i64,
    name: String,
    channel_type: String,
    config: Value,
    events: Vec<String>,
    enabled: bool,
) -> anyhow::Result<Option<Integration>> {
    let config_json = serde_json::to_string(&config)?;
    let events_json = serde_json::to_string(&events)?;
    let enabled_int = if enabled { 1 } else { 0 };

    let n = name.clone();
    let ct = channel_type.clone();
    let rows_affected = db
        .call(move |conn| {
            let affected = conn.execute(
                "UPDATE integrations SET name = ?1, channel_type = ?2, config_json = ?3, events_json = ?4, enabled = ?5 WHERE id = ?6",
                params![n, ct, config_json, events_json, enabled_int, id],
            )?;
            Ok(affected)
        })
        .await?;

    if rows_affected == 0 {
        return Ok(None);
    }

    Ok(Some(Integration {
        id,
        name,
        channel_type,
        config,
        events,
        enabled,
    }))
}

pub async fn delete(db: &Db, id: i64) -> anyhow::Result<bool> {
    let affected = db
        .call(move |conn| {
            let aff = conn.execute("DELETE FROM integrations WHERE id = ?1", params![id])?;
            Ok(aff)
        })
        .await?;
    Ok(affected > 0)
}

pub async fn list_enabled_for_event(db: &Db, event_name: &str) -> anyhow::Result<Vec<Integration>> {
    let all = list(db).await?;
    let filtered = all
        .into_iter()
        .filter(|i| i.enabled && i.events.iter().any(|e| e == event_name))
        .collect();
    Ok(filtered)
}

pub async fn dispatch_event(db: &Db, client: &reqwest::Client, event: &IntegrationEvent) {
    let event_name = event.event_name();
    let integrations = match list_enabled_for_event(db, event_name).await {
        Ok(list) => list,
        Err(e) => {
            tracing::error!(error = %e, event = %event_name, "failed to query integrations for event");
            return;
        }
    };

    for integration in integrations {
        let client = client.clone();
        let event = event.clone();
        tokio::spawn(async move {
            if let Err(e) = channels::dispatch(
                &integration.channel_type,
                &client,
                &integration.config,
                &event,
            )
            .await
            {
                tracing::warn!(
                    integration_id = integration.id,
                    integration_name = %integration.name,
                    channel_type = %integration.channel_type,
                    error = %e,
                    "failed to deliver notification"
                );
            }
        });
    }
}

pub struct IntegrationDispatcher {
    db: Db,
    runner_rx: broadcast::Receiver<RunEvent>,
    reload_rx: watch::Receiver<()>,
    client: reqwest::Client,
}

impl IntegrationDispatcher {
    pub fn new(
        db: Db,
        runner_rx: broadcast::Receiver<RunEvent>,
        reload_rx: watch::Receiver<()>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            db,
            runner_rx,
            reload_rx,
            client,
        }
    }

    pub fn spawn(mut self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(60));
            ticker.tick().await; // Consume immediate first tick

            loop {
                tokio::select! {
                    event = self.runner_rx.recv() => {
                        match event {
                            Ok(run_event) => {
                                let now = chrono::Utc::now().timestamp_millis();
                                match run_event {
                                    RunEvent::Started { kind } => {
                                        let kind_str = match kind {
                                            TestKind::Auto => "auto",
                                            TestKind::Custom => "custom",
                                        };
                                        let ev = IntegrationEvent::SpeedtestStarted {
                                            kind: kind_str.to_string(),
                                            timestamp: now,
                                        };
                                        dispatch_event(&self.db, &self.client, &ev).await;
                                    }
                                    RunEvent::Finished { id, metrics } => {
                                        let (server_name, server_id, server_host, provider) = match crate::db::tests_repo::get(&self.db, id).await {
                                            Ok(Some(row)) => (row.server_name, row.server_id, row.server_host, Some(row.provider)),
                                            _ => (None, None, None, None),
                                        };
                                        let ev = IntegrationEvent::SpeedtestFinished {
                                            id,
                                            metrics,
                                            server_name,
                                            server_id,
                                            server_host,
                                            provider,
                                            timestamp: now,
                                        };
                                        dispatch_event(&self.db, &self.client, &ev).await;
                                    }
                                    RunEvent::Failed { id, error } => {
                                        let ev = IntegrationEvent::SpeedtestFailed {
                                            id,
                                            error,
                                            timestamp: now,
                                        };
                                        dispatch_event(&self.db, &self.client, &ev).await;
                                    }
                                    RunEvent::ExpectedUpdated { ping, download, upload } => {
                                        let ev = IntegrationEvent::ExpectedUpdated {
                                            ping,
                                            download,
                                            upload,
                                            timestamp: now,
                                        };
                                        dispatch_event(&self.db, &self.client, &ev).await;
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                tracing::warn!("integration dispatcher lagged by {n} runner events");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                tracing::info!("runner event channel closed; ending integration dispatcher");
                                break;
                            }
                        }
                    }
                    changed = self.reload_rx.changed() => {
                        if changed.is_ok() {
                            let ev = IntegrationEvent::ConfigUpdated {
                                timestamp: chrono::Utc::now().timestamp_millis(),
                            };
                            dispatch_event(&self.db, &self.client, &ev).await;
                        } else {
                            break;
                        }
                    }
                    _ = ticker.tick() => {
                        let ev = IntegrationEvent::MinutePassed {
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        };
                        dispatch_event(&self.db, &self.client, &ev).await;
                    }
                }
            }
        })
    }
}
