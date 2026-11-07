use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState};
use crate::net::{self, NetInterface};
use crate::providers::LibreServer;
use crate::providers::librespeed::fetch_libre_servers;
use crate::system::process_stats;

const LIBRE_CACHE_TTL: Duration = Duration::from_secs(3600);

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/status", get(status))
        .route("/api/system/interfaces", get(interfaces))
        .route("/api/system/servers/librespeed", get(libre_servers))
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    let stats = process_stats();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptimeSecs": s.started_at.elapsed().as_secs(),
        "cpuPercent": stats.cpu_percent,
        "rssBytes": stats.rss_bytes,
        "tz": s.db.tz().name(),
    }))
}

async fn interfaces() -> Json<Vec<NetInterface>> {
    Json(net::list_interfaces())
}

async fn libre_servers(State(s): State<AppState>) -> ApiResult<Json<Vec<LibreServer>>> {
    {
        let cache = s.libre_cache.lock().unwrap();
        if let Some((at, servers)) = cache.as_ref()
            && at.elapsed() < LIBRE_CACHE_TTL
        {
            return Ok(Json(servers.clone()));
        }
    }
    let client = net::http_client(None);
    let servers = fetch_libre_servers(&client, &s.libre_list_url)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("cannot fetch librespeed server list: {e}"),
            )
        })?;
    *s.libre_cache.lock().unwrap() = Some((Instant::now(), servers.clone()));
    Ok(Json(servers))
}
