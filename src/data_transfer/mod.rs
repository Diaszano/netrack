pub mod routes;

use chrono::NaiveDateTime;
use serde_json::Value;

use crate::db::{Db, local_fields};

#[derive(Debug, Clone)]
pub struct ImportRecord {
    pub created_at: i64,
    pub kind: String,
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

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn escape_csv(val: &str) -> String {
    if val.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", val.replace('"', "\"\""))
    } else {
        val.to_string()
    }
}

pub fn parse_timestamp(val: &Value) -> Result<i64, String> {
    match val {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                if i < 100_000_000_000 {
                    Ok(i * 1000)
                } else {
                    Ok(i)
                }
            } else if let Some(f) = n.as_f64() {
                let i = f as i64;
                if i < 100_000_000_000 {
                    Ok(i * 1000)
                } else {
                    Ok(i)
                }
            } else {
                Err("invalid numeric timestamp".into())
            }
        }
        Value::String(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Err("empty timestamp string".into());
            }
            if let Ok(i) = s.parse::<i64>() {
                return if i < 100_000_000_000 {
                    Ok(i * 1000)
                } else {
                    Ok(i)
                };
            }
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                return Ok(dt.timestamp_millis());
            }
            for fmt in &[
                "%Y-%m-%d %H:%M:%S%.f",
                "%Y-%m-%d %H:%M:%S",
                "%Y-%m-%dT%H:%M:%S%.f",
                "%Y-%m-%dT%H:%M:%S",
            ] {
                if let Ok(naive) = NaiveDateTime::parse_from_str(s, fmt) {
                    return Ok(naive.and_utc().timestamp_millis());
                }
            }
            if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                && let Some(naive) = naive_date.and_hms_opt(0, 0, 0)
            {
                return Ok(naive.and_utc().timestamp_millis());
            }
            Err(format!("cannot parse date string: {s}"))
        }
        _ => Err("expected string or number for timestamp".into()),
    }
}

fn parse_opt_f64(val: Option<&Value>) -> Result<Option<f64>, String> {
    match val {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => {
            if let Some(f) = n.as_f64() {
                Ok(Some(round2(f)))
            } else {
                Err("invalid number".into())
            }
        }
        Some(Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse::<f64>()
                    .map(|f| Some(round2(f)))
                    .map_err(|e| format!("invalid number string {s:?}: {e}"))
            }
        }
        _ => Err("expected number, string, or null".into()),
    }
}

fn parse_opt_string(val: Option<&Value>) -> Option<String> {
    match val {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

pub fn parse_json_records(value: &Value) -> Result<Vec<ImportRecord>, String> {
    let records: &[Value] = match value {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(map) => {
            if let Some(Value::Array(arr)) = map
                .get("data")
                .or_else(|| map.get("speedtests"))
                .or_else(|| map.get("tests"))
            {
                arr.as_slice()
            } else {
                return Err("expected a JSON array of speedtest records".into());
            }
        }
        _ => return Err("expected a JSON array of speedtest records".into()),
    };

    let mut result = Vec::with_capacity(records.len());
    for (idx, item) in records.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("item at index {idx} is not an object"))?;

        let is_myspeed = obj.contains_key("createdOn")
            || (obj.contains_key("server") && !obj.contains_key("kind"));

        let created_at_val = if is_myspeed {
            obj.get("createdOn")
                .or_else(|| obj.get("created"))
                .or_else(|| obj.get("createdAt"))
                .or_else(|| obj.get("timestamp"))
                .or_else(|| obj.get("time"))
        } else {
            obj.get("createdAt")
                .or_else(|| obj.get("created_at"))
                .or_else(|| obj.get("created"))
                .or_else(|| obj.get("createdOn"))
        };

        let created_at_val =
            created_at_val.ok_or_else(|| format!("record at index {idx} is missing timestamp"))?;
        let created_at = parse_timestamp(created_at_val)
            .map_err(|e| format!("record at index {idx} has invalid timestamp: {e}"))?;

        let kind = obj
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("auto")
            .to_string();

        let provider = if is_myspeed {
            obj.get("type")
                .or_else(|| obj.get("provider"))
                .and_then(|v| v.as_str())
                .unwrap_or("myspeed")
                .to_string()
        } else {
            obj.get("provider")
                .or_else(|| obj.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string()
        };

        let (mut server_id, mut server_name, mut server_host) = (None, None, None);

        if let Some(srv) = obj.get("server") {
            if let Some(srv_obj) = srv.as_object() {
                server_id = srv_obj.get("id").map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                });
                server_name = srv_obj
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string);
                server_host = srv_obj
                    .get("host")
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string);
            } else if let Some(srv_str) = srv.as_str() {
                server_name = Some(srv_str.to_string());
            }
        }

        if server_id.is_none() {
            server_id = parse_opt_string(obj.get("serverId").or_else(|| obj.get("server_id")));
        }
        if server_name.is_none() {
            server_name =
                parse_opt_string(obj.get("serverName").or_else(|| obj.get("server_name")));
        }
        if server_host.is_none() {
            server_host =
                parse_opt_string(obj.get("serverHost").or_else(|| obj.get("server_host")));
        }

        let result_id = parse_opt_string(obj.get("resultId").or_else(|| obj.get("result_id")));

        let ping = parse_opt_f64(obj.get("ping"))
            .map_err(|e| format!("record at index {idx} has invalid ping: {e}"))?;
        let jitter = parse_opt_f64(obj.get("jitter"))
            .map_err(|e| format!("record at index {idx} has invalid jitter: {e}"))?;

        let mut download = parse_opt_f64(obj.get("download"))
            .map_err(|e| format!("record at index {idx} has invalid download: {e}"))?;
        if let Some(ref mut dl) = download
            && *dl > 100_000.0
        {
            *dl = round2(*dl / 1_000_000.0);
        }

        let mut upload = parse_opt_f64(obj.get("upload"))
            .map_err(|e| format!("record at index {idx} has invalid upload: {e}"))?;
        if let Some(ref mut ul) = upload
            && *ul > 100_000.0
        {
            *ul = round2(*ul / 1_000_000.0);
        }

        let mut duration = parse_opt_f64(
            obj.get("duration")
                .or_else(|| if is_myspeed { obj.get("time") } else { None }),
        )
        .map_err(|e| format!("record at index {idx} has invalid duration: {e}"))?;

        if let Some(ref mut d) = duration
            && *d > 100.0
        {
            *d = round2(*d / 1000.0);
        }

        let error = parse_opt_string(obj.get("error"));

        result.push(ImportRecord {
            created_at,
            kind,
            provider,
            server_id,
            server_name,
            server_host,
            result_id,
            ping,
            jitter,
            download,
            upload,
            duration,
            error,
        });
    }

    Ok(result)
}

pub fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(field);
            field = String::new();
        } else {
            field.push(c);
        }
    }
    fields.push(field);
    fields
}

pub fn parse_csv_records(content: &str) -> Result<Vec<ImportRecord>, String> {
    let mut lines = content.lines();
    let header = match lines.next() {
        Some(h) => h.trim(),
        None => return Ok(Vec::new()),
    };

    if !header.contains("created_at") {
        return Err("CSV header must contain 'created_at'".into());
    }

    let header_cols = parse_csv_line(header);
    let mut result = Vec::new();

    for (idx, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols = parse_csv_line(line);
        let get = |col_name: &str| -> Option<&str> {
            header_cols
                .iter()
                .position(|h| h.trim() == col_name)
                .and_then(|i| cols.get(i))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        };

        let created_at_str =
            get("created_at").ok_or_else(|| format!("CSV line {idx} missing created_at"))?;
        let created_at = parse_timestamp(&Value::String(created_at_str.to_string()))
            .map_err(|e| format!("CSV line {idx} invalid created_at: {e}"))?;

        let kind = get("kind").unwrap_or("auto").to_string();
        let provider = get("provider").unwrap_or("unknown").to_string();
        let server_id = get("server_id").map(ToString::to_string);
        let server_name = get("server_name").map(ToString::to_string);
        let server_host = get("server_host").map(ToString::to_string);
        let result_id = get("result_id").map(ToString::to_string);

        let ping = get("ping")
            .map(|s| s.parse::<f64>().map(round2))
            .transpose()
            .map_err(|e| format!("CSV line {idx} invalid ping: {e}"))?;
        let jitter = get("jitter")
            .map(|s| s.parse::<f64>().map(round2))
            .transpose()
            .map_err(|e| format!("CSV line {idx} invalid jitter: {e}"))?;
        let download = get("download")
            .map(|s| s.parse::<f64>().map(round2))
            .transpose()
            .map_err(|e| format!("CSV line {idx} invalid download: {e}"))?;
        let upload = get("upload")
            .map(|s| s.parse::<f64>().map(round2))
            .transpose()
            .map_err(|e| format!("CSV line {idx} invalid upload: {e}"))?;
        let duration = get("duration")
            .map(|s| s.parse::<f64>().map(round2))
            .transpose()
            .map_err(|e| format!("CSV line {idx} invalid duration: {e}"))?;
        let error = get("error").map(ToString::to_string);

        result.push(ImportRecord {
            created_at,
            kind,
            provider,
            server_id,
            server_name,
            server_host,
            result_id,
            ping,
            jitter,
            download,
            upload,
            duration,
            error,
        });
    }

    Ok(result)
}

pub async fn insert_records(db: &Db, records: Vec<ImportRecord>) -> anyhow::Result<usize> {
    let tz = db.tz();
    let count = records.len();
    db.call(move |conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO speedtests (
                    created_at, local_hour, local_weekday, kind, provider,
                    server_id, server_name, server_host, result_id,
                    ping, jitter, download, upload, duration, error
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            )?;
            for r in records {
                let (local_hour, local_weekday) = local_fields(tz, r.created_at);
                stmt.execute(rusqlite::params![
                    r.created_at,
                    local_hour,
                    local_weekday,
                    r.kind,
                    r.provider,
                    r.server_id,
                    r.server_name,
                    r.server_host,
                    r.result_id,
                    r.ping,
                    r.jitter,
                    r.download,
                    r.upload,
                    r.duration,
                    r.error,
                ])?;
            }
        }
        tx.commit()?;
        Ok(count)
    })
    .await
}

pub async fn reset_database(db: &Db) -> anyhow::Result<()> {
    db.call(move |conn| {
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM speedtests", [])?;
        let _ = tx.execute("DELETE FROM sqlite_sequence WHERE name = 'speedtests'", []);
        tx.execute("DELETE FROM config", [])?;
        for (k, v) in crate::db::config::DEFAULTS {
            tx.execute("INSERT INTO config (key, value) VALUES (?1, ?2)", [*k, *v])?;
        }
        tx.commit()?;
        Ok(())
    })
    .await
}

pub async fn get_data_stats(db: &Db, data_dir: &std::path::Path) -> anyhow::Result<(u64, u64)> {
    let (row_count, page_count, page_size) = db
        .call(|conn| {
            let row_count: i64 =
                conn.query_row("SELECT COUNT(*) FROM speedtests", [], |r| r.get(0))?;
            let page_count: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
            let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
            Ok((row_count, page_count, page_size))
        })
        .await?;

    let db_file = data_dir.join("netrack.db");
    let db_size_bytes = if let Ok(meta) = std::fs::metadata(&db_file) {
        meta.len()
    } else {
        (page_count.max(0) * page_size.max(0)) as u64
    };

    Ok((row_count.max(0) as u64, db_size_bytes))
}
