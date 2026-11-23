use axum::extract::{DefaultBodyLimit, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    escape_csv, get_data_stats, insert_records, parse_csv_records, parse_json_records,
    reset_database,
};
use crate::api::{ApiError, ApiPath, ApiQuery, ApiResult, AppState, parse_date};
use crate::db::backup;
use crate::db::stats::local_midnight_ms;
use crate::db::tests_repo;

const MAX_IMPORT_BYTES: usize = 64 * 1024 * 1024;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/data/export", get(export_data))
        .route(
            "/api/data/import",
            post(import_data).layer(DefaultBodyLimit::max(MAX_IMPORT_BYTES)),
        )
        .route("/api/data/stats", get(data_stats))
        .route("/api/data/backups/{name}", get(download_backup))
        .route("/api/data/reset", post(data_reset))
}

#[derive(Debug, Deserialize)]
struct ExportQuery {
    format: Option<String>,
    /// Optional local-date range (`YYYY-MM-DD`, both ends inclusive), like the statistics API.
    from: Option<String>,
    to: Option<String>,
}

async fn export_data(
    State(s): State<AppState>,
    ApiQuery(query): ApiQuery<ExportQuery>,
) -> ApiResult<Response> {
    let format = query.format.as_deref().unwrap_or("json").to_lowercase();
    let tz = s.db.tz();
    let bound = |name: &str, v: Option<&str>, next_day: bool| -> ApiResult<Option<i64>> {
        let Some(v) = v else { return Ok(None) };
        let mut day = parse_date(name, Some(v))?;
        if next_day {
            day = day
                .succ_opt()
                .ok_or_else(|| ApiError::bad_request(format!("{name} is out of range")))?;
        }
        Ok(Some(local_midnight_ms(tz, day)?))
    };
    let from_ms = bound("from", query.from.as_deref(), false)?;
    let to_ms = bound("to", query.to.as_deref(), true)?;
    let mut rows = tests_repo::all(&s.db).await?;
    rows.retain(|r| {
        from_ms.is_none_or(|f| r.created_at >= f) && to_ms.is_none_or(|t| r.created_at < t)
    });
    let timestamp = Utc::now().timestamp();

    match format.as_str() {
        "json" => {
            let json_str = serde_json::to_string(&rows)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            let filename = format!("netrack-export-{timestamp}.json");
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                )
                .body(axum::body::Body::from(json_str))
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        "csv" => {
            let mut csv_str = String::from(
                "id,created_at,kind,provider,server_id,server_name,server_host,result_id,ping,jitter,download,upload,duration,error\n",
            );
            for row in rows {
                csv_str.push_str(&format!(
                    "{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                    row.id,
                    row.created_at,
                    row.kind.as_str(),
                    escape_csv(&row.provider),
                    row.server_id.as_deref().map(escape_csv).unwrap_or_default(),
                    row.server_name
                        .as_deref()
                        .map(escape_csv)
                        .unwrap_or_default(),
                    row.server_host
                        .as_deref()
                        .map(escape_csv)
                        .unwrap_or_default(),
                    row.result_id.as_deref().map(escape_csv).unwrap_or_default(),
                    row.ping.map(|v| v.to_string()).unwrap_or_default(),
                    row.jitter.map(|v| v.to_string()).unwrap_or_default(),
                    row.download.map(|v| v.to_string()).unwrap_or_default(),
                    row.upload.map(|v| v.to_string()).unwrap_or_default(),
                    row.duration.map(|v| v.to_string()).unwrap_or_default(),
                    row.error.as_deref().map(escape_csv).unwrap_or_default(),
                ));
            }
            let filename = format!("netrack-export-{timestamp}.csv");
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/csv; charset=utf-8")
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{filename}\""),
                )
                .body(axum::body::Body::from(csv_str))
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
        _ => Err(ApiError::bad_request(
            "unsupported format, expected 'json' or 'csv'",
        )),
    }
}

async fn import_data(State(s): State<AppState>, body: axum::body::Bytes) -> ApiResult<Json<Value>> {
    let records = match serde_json::from_slice::<Value>(&body) {
        Ok(val) => parse_json_records(&val).map_err(ApiError::bad_request)?,
        Err(json_err) => {
            if let Ok(text) = std::str::from_utf8(&body) {
                if text.trim().starts_with("id,created_at")
                    || text
                        .lines()
                        .next()
                        .is_some_and(|l| l.contains("created_at"))
                {
                    parse_csv_records(text).map_err(ApiError::bad_request)?
                } else {
                    return Err(ApiError::bad_request(format!("invalid JSON: {json_err}")));
                }
            } else {
                return Err(ApiError::bad_request(format!("invalid JSON: {json_err}")));
            }
        }
    };

    let out = insert_records(&s.db, records)
        .await
        .map_err(|e| ApiError::bad_request(format!("failed to import records: {e}")))?;

    Ok(Json(
        json!({ "imported": out.imported, "skipped": out.skipped }),
    ))
}

async fn data_stats(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let (row_count, db_size_bytes) = get_data_stats(&s.db, &s.data_dir).await?;
    let backups: Vec<Value> = backup::list(&s.data_dir)
        .await?
        .into_iter()
        .map(|(name, size)| json!({ "name": name, "sizeBytes": size }))
        .collect();
    Ok(Json(json!({
        "rowCount": row_count,
        "dbSizeBytes": db_size_bytes,
        "backups": backups,
    })))
}

async fn download_backup(
    State(s): State<AppState>,
    ApiPath(name): ApiPath<String>,
) -> ApiResult<Response> {
    let not_found = || ApiError::new(StatusCode::NOT_FOUND, "backup not found");
    if !backup::is_backup_name(&name) {
        return Err(not_found());
    }
    // shortcut: reads the whole file into memory; fine for SQLite files of a few MB, stream it if they grow past ~100 MB.
    let bytes = match tokio::fs::read(s.data_dir.join("backups").join(&name)).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(anyhow::Error::from(e).into()),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.sqlite3")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        )
        .body(axum::body::Body::from(bytes))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

async fn data_reset(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    reset_database(&s.db).await?;
    s.ping_reload();
    Ok(Json(json!({ "reset": true })))
}
