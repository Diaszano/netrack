use anyhow::{Context, bail};
use chrono::{Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use rusqlite::{Connection, params};
use serde::Serialize;

use super::Db;

const MAX_SERIES_POINTS: i64 = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Statistics {
    pub total: u64,
    pub failed: u64,
    /// Percentage of non-failed tests, 1 decimal.
    pub availability: f64,
    pub download: MetricSummary,
    pub upload: MetricSummary,
    pub ping: MetricSummary,
    pub jitter: MetricSummary,
    /// Always 24 entries (hour 0..23); metrics are `None` for empty hours.
    pub hourly: Vec<HourBucket>,
    /// Only weekday/hour cells that have data.
    pub heatmap: Vec<HeatCell>,
    /// At most 300 points, oldest first.
    pub series: Vec<SeriesPoint>,
    /// One row per provider and server, most tests first.
    pub by_server: Vec<ServerStat>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStat {
    pub provider: String,
    /// `None` for failed tests that never reached a server.
    pub server_name: Option<String>,
    pub total: u64,
    pub failed: u64,
    /// Averages of successful tests only.
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub ping: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricSummary {
    pub avg: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// Population standard deviation.
    pub std_dev: Option<f64>,
    /// `100 - std_dev / avg * 100`, 1 decimal; 100 when avg is 0.
    pub consistency: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourBucket {
    pub hour: u8,
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub ping: Option<f64>,
    pub jitter: Option<f64>,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatCell {
    pub weekday: u8,
    pub hour: u8,
    pub download: f64,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesPoint {
    /// Unix ms (UTC); bucket start when downsampled.
    pub t: i64,
    pub download: Option<f64>,
    pub upload: Option<f64>,
    pub ping: Option<f64>,
}

fn round_to(v: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (v * f).round() / f
}

fn round2(v: Option<f64>) -> Option<f64> {
    v.map(|v| round_to(v, 2))
}

/// Start of `date` in `tz` as unix ms. Ambiguous midnights resolve to the earlier
/// instant; a nonexistent midnight (DST gap) resolves to the first valid instant after it.
pub(crate) fn local_midnight_ms(tz: Tz, date: NaiveDate) -> anyhow::Result<i64> {
    let midnight = date.and_hms_opt(0, 0, 0).context("invalid date")?;
    for minutes in 0..=(48 * 60) {
        match tz.from_local_datetime(&(midnight + Duration::minutes(minutes))) {
            LocalResult::Single(t) => return Ok(t.with_timezone(&Utc).timestamp_millis()),
            LocalResult::Ambiguous(earliest, _) => {
                return Ok(earliest.with_timezone(&Utc).timestamp_millis());
            }
            LocalResult::None => {}
        }
    }
    bail!("no valid local time on {date} in {tz}")
}

/// Statistics over the inclusive whole-day range `from..=to` in the server timezone.
pub async fn statistics(db: &Db, from: NaiveDate, to: NaiveDate) -> anyhow::Result<Statistics> {
    if to < from {
        bail!("range end {to} is before start {from}");
    }
    let tz = db.tz();
    let from_ms = local_midnight_ms(tz, from)?;
    let to_ms = local_midnight_ms(tz, to.succ_opt().context("date out of range")?)?;
    db.call(move |conn| query(conn, from_ms, to_ms)).await
}

#[derive(Debug, Clone, Default)]
pub struct Window24h {
    pub ok: u64,
    pub failed: u64,
    pub avg_download: Option<f64>,
    pub avg_upload: Option<f64>,
    pub avg_ping: Option<f64>,
}

/// Counts and averages (OK tests only) for tests created at or after `since_ms`.
pub async fn window(db: &Db, since_ms: i64) -> anyhow::Result<Window24h> {
    db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FILTER (WHERE error IS NULL), COUNT(error), \
             AVG(download) FILTER (WHERE error IS NULL), \
             AVG(upload) FILTER (WHERE error IS NULL), \
             AVG(ping) FILTER (WHERE error IS NULL) \
             FROM speedtests WHERE created_at >= ?1",
            [since_ms],
            |r| {
                Ok(Window24h {
                    ok: r.get::<_, i64>(0)? as u64,
                    failed: r.get::<_, i64>(1)? as u64,
                    avg_download: r.get(2)?,
                    avg_upload: r.get(3)?,
                    avg_ping: r.get(4)?,
                })
            },
        )?)
    })
    .await
}

fn query(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<Statistics> {
    let (total, failed) = conn.query_row(
        "SELECT COUNT(*), COUNT(error) FROM speedtests WHERE created_at >= ?1 AND created_at < ?2",
        [from_ms, to_ms],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    )?;
    let availability = if total == 0 {
        100.0
    } else {
        round_to((total - failed) as f64 / total as f64 * 100.0, 1)
    };

    let [download, upload, ping, jitter] = summaries(conn, from_ms, to_ms)?;
    Ok(Statistics {
        total: total as u64,
        failed: failed as u64,
        availability,
        download,
        upload,
        ping,
        jitter,
        hourly: hourly(conn, from_ms, to_ms)?,
        heatmap: heatmap(conn, from_ms, to_ms)?,
        series: series(conn, from_ms, to_ms)?,
        by_server: by_server(conn, from_ms, to_ms)?,
    })
}

fn by_server(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<Vec<ServerStat>> {
    let mut stmt = conn.prepare(
        "SELECT provider, server_name, COUNT(*), COUNT(error), \
         AVG(download) FILTER (WHERE error IS NULL), \
         AVG(upload) FILTER (WHERE error IS NULL), \
         AVG(ping) FILTER (WHERE error IS NULL) \
         FROM speedtests WHERE created_at >= ?1 AND created_at < ?2 \
         GROUP BY provider, server_name ORDER BY COUNT(*) DESC, provider, server_name",
    )?;
    let rows = stmt.query_map([from_ms, to_ms], |r| {
        Ok(ServerStat {
            provider: r.get(0)?,
            server_name: r.get(1)?,
            total: r.get::<_, i64>(2)? as u64,
            failed: r.get::<_, i64>(3)? as u64,
            download: round2(r.get(4)?),
            upload: round2(r.get(5)?),
            ping: round2(r.get(6)?),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn summaries(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<[MetricSummary; 4]> {
    let cols = ["download", "upload", "ping", "jitter"]
        .iter()
        .map(|c| {
            format!(
                "AVG({c}), MIN({c}), MAX({c}), \
                 MAX(0.0, AVG({c} * {c}) - AVG({c}) * AVG({c}))"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {cols} FROM speedtests \
         WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL"
    );
    Ok(conn.query_row(&sql, [from_ms, to_ms], |r| {
        let mut out: [MetricSummary; 4] = Default::default();
        for (i, slot) in out.iter_mut().enumerate() {
            let avg: Option<f64> = r.get(i * 4)?;
            // Variance is computed (and clamped at 0) in SQL; the bundled SQLite has no SQRT.
            let std_dev = r.get::<_, Option<f64>>(i * 4 + 3)?.map(f64::sqrt);
            let consistency = avg.zip(std_dev).map(|(avg, sd)| {
                if avg == 0.0 {
                    100.0
                } else {
                    round_to(100.0 - sd / avg * 100.0, 1)
                }
            });
            *slot = MetricSummary {
                avg: round2(avg),
                min: round2(r.get(i * 4 + 1)?),
                max: round2(r.get(i * 4 + 2)?),
                std_dev: round2(std_dev),
                consistency,
            };
        }
        Ok(out)
    })?)
}

fn hourly(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<Vec<HourBucket>> {
    let mut buckets: Vec<HourBucket> = (0..24u8)
        .map(|hour| HourBucket {
            hour,
            download: None,
            upload: None,
            ping: None,
            jitter: None,
            count: 0,
        })
        .collect();
    let mut stmt = conn.prepare(
        "SELECT local_hour, AVG(download), AVG(upload), AVG(ping), AVG(jitter), COUNT(*) \
         FROM speedtests WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL \
         GROUP BY local_hour",
    )?;
    let rows = stmt.query_map([from_ms, to_ms], |r| {
        Ok((
            r.get::<_, u32>(0)?,
            [r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?],
            r.get::<_, i64>(5)?,
        ))
    })?;
    for row in rows {
        let (hour, [d, u, p, j], count): (u32, [Option<f64>; 4], i64) = row?;
        if let Some(b) = buckets.get_mut(hour as usize) {
            b.download = round2(d);
            b.upload = round2(u);
            b.ping = round2(p);
            b.jitter = round2(j);
            b.count = count as u64;
        }
    }
    Ok(buckets)
}

fn heatmap(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<Vec<HeatCell>> {
    let mut stmt = conn.prepare(
        "SELECT local_weekday, local_hour, AVG(download), COUNT(*) FROM speedtests \
         WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL \
         GROUP BY local_weekday, local_hour ORDER BY local_weekday, local_hour",
    )?;
    let cells = stmt
        .query_map([from_ms, to_ms], |r| {
            Ok(HeatCell {
                weekday: r.get(0)?,
                hour: r.get(1)?,
                download: round_to(r.get::<_, f64>(2)?, 2),
                count: r.get::<_, i64>(3)? as u64,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(cells)
}

fn series(conn: &Connection, from_ms: i64, to_ms: i64) -> anyhow::Result<Vec<SeriesPoint>> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM speedtests \
         WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL",
        [from_ms, to_ms],
        |r| r.get(0),
    )?;
    let map = |r: &rusqlite::Row| -> rusqlite::Result<SeriesPoint> {
        Ok(SeriesPoint {
            t: r.get(0)?,
            download: round2(r.get(1)?),
            upload: round2(r.get(2)?),
            ping: round2(r.get(3)?),
        })
    };
    let points = if n <= MAX_SERIES_POINTS {
        conn.prepare(
            "SELECT created_at, download, upload, ping FROM speedtests \
             WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL ORDER BY created_at",
        )?
        .query_map([from_ms, to_ms], map)?
        .collect::<Result<_, _>>()?
    } else {
        let width = ((to_ms - from_ms) + MAX_SERIES_POINTS - 1) / MAX_SERIES_POINTS;
        conn.prepare(
            "SELECT ?1 + ((created_at - ?1) / ?3) * ?3, AVG(download), AVG(upload), AVG(ping) \
             FROM speedtests WHERE created_at >= ?1 AND created_at < ?2 AND error IS NULL \
             GROUP BY (created_at - ?1) / ?3 ORDER BY 1",
        )?
        .query_map(params![from_ms, to_ms, width.max(1)], map)?
        .collect::<Result<_, _>>()?
    };
    Ok(points)
}
