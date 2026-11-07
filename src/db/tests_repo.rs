use anyhow::anyhow;
use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::{Db, local_fields};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestKind {
    Auto,
    Custom,
}

impl TestKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TestKind::Auto => "auto",
            TestKind::Custom => "custom",
        }
    }

    pub fn parse(s: &str) -> Option<TestKind> {
        match s {
            "auto" => Some(TestKind::Auto),
            "custom" => Some(TestKind::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeedtestRow {
    pub id: i64,
    pub created_at: i64,
    pub kind: TestKind,
    pub provider: String,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
    pub server_host: Option<String>,
    pub result_id: Option<String>,
    pub ping: Option<f64>,
    pub jitter: Option<f64>,
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub duration: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Metrics {
    pub ping: f64,
    pub jitter: f64,
    pub download: f64,
    pub upload: f64,
}

#[derive(Debug, Clone)]
pub struct NewSpeedtest {
    pub kind: TestKind,
    pub provider: String,
    pub server_id: Option<String>,
    pub server_name: Option<String>,
    pub server_host: Option<String>,
    pub result_id: Option<String>,
    pub metrics: Result<Metrics, String>,
    pub duration: f64,
}

const COLUMNS: &str = "id, created_at, kind, provider, server_id, server_name, server_host, \
                       result_id, ping, jitter, download, upload, duration, error";

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn map_row(r: &Row) -> rusqlite::Result<SpeedtestRow> {
    let kind: String = r.get(2)?;
    let kind = TestKind::parse(&kind).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            format!("unknown test kind {kind:?}").into(),
        )
    })?;
    Ok(SpeedtestRow {
        id: r.get(0)?,
        created_at: r.get(1)?,
        kind,
        provider: r.get(3)?,
        server_id: r.get(4)?,
        server_name: r.get(5)?,
        server_host: r.get(6)?,
        result_id: r.get(7)?,
        ping: r.get(8)?,
        jitter: r.get(9)?,
        download: r.get(10)?,
        upload: r.get(11)?,
        duration: r.get(12)?,
        error: r.get(13)?,
    })
}

pub async fn insert(db: &Db, t: NewSpeedtest, at: DateTime<Utc>) -> anyhow::Result<i64> {
    let created_at = at.timestamp_millis();
    let (hour, weekday) = local_fields(db.tz(), created_at);
    let (metrics, error) = match t.metrics {
        Ok(m) => (Some(m), None),
        Err(e) => (None, Some(e)),
    };
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO speedtests (created_at, local_hour, local_weekday, kind, provider, \
             server_id, server_name, server_host, result_id, ping, jitter, download, upload, \
             duration, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                created_at,
                hour,
                weekday,
                t.kind.as_str(),
                t.provider,
                t.server_id,
                t.server_name,
                t.server_host,
                t.result_id,
                metrics.map(|m| round2(m.ping)),
                metrics.map(|m| round2(m.jitter)),
                metrics.map(|m| round2(m.download)),
                metrics.map(|m| round2(m.upload)),
                round2(t.duration),
                error,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    })
    .await
}

/// All stored speedtests, oldest first (`ORDER BY id ASC`).
pub async fn all(db: &Db) -> anyhow::Result<Vec<SpeedtestRow>> {
    db.call(move |conn| {
        let mut stmt =
            conn.prepare(&format!("SELECT {COLUMNS} FROM speedtests ORDER BY id ASC"))?;
        let rows = stmt
            .query_map([], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
}

/// Newest first; `before_id` is an exclusive cursor; `limit` is clamped to 1..=100.
pub async fn list(
    db: &Db,
    before_id: Option<i64>,
    limit: u32,
) -> anyhow::Result<Vec<SpeedtestRow>> {
    let limit = limit.clamp(1, 100);
    db.call(move |conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM speedtests WHERE (?1 IS NULL OR id < ?1) ORDER BY id DESC LIMIT ?2"
        ))?;
        let rows = stmt
            .query_map(params![before_id, limit], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> anyhow::Result<Option<SpeedtestRow>> {
    db.call(move |conn| {
        Ok(conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM speedtests WHERE id = ?1"),
                [id],
                map_row,
            )
            .optional()?)
    })
    .await
}

pub async fn delete(db: &Db, id: i64) -> anyhow::Result<bool> {
    db.call(move |conn| Ok(conn.execute("DELETE FROM speedtests WHERE id = ?1", [id])? > 0))
        .await
}

pub async fn delete_older_than(db: &Db, days: u32, now: DateTime<Utc>) -> anyhow::Result<u64> {
    let cutoff = now
        .checked_sub_signed(Duration::days(i64::from(days)))
        .ok_or_else(|| anyhow!("retention cutoff out of range"))?
        .timestamp_millis();
    db.call(move |conn| {
        Ok(conn.execute("DELETE FROM speedtests WHERE created_at < ?1", [cutoff])? as u64)
    })
    .await
}

/// The `n` most recent tests that completed without error, newest first.
pub async fn last_successful(db: &Db, n: u32) -> anyhow::Result<Vec<SpeedtestRow>> {
    db.call(move |conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM speedtests WHERE error IS NULL ORDER BY id DESC LIMIT ?1"
        ))?;
        let rows = stmt
            .query_map([n], map_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
    .await
}

/// `created_at` of the newest stored test, if any.
pub async fn newest_created_at(db: &Db) -> anyhow::Result<Option<DateTime<Utc>>> {
    let ms: Option<i64> = db
        .call(
            |conn| Ok(conn.query_row("SELECT MAX(created_at) FROM speedtests", [], |r| r.get(0))?),
        )
        .await?;
    Ok(ms.and_then(|ms| Utc.timestamp_millis_opt(ms).single()))
}
