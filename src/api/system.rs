use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use super::{ApiError, ApiResult, AppState};
use crate::db::config;
use crate::net::{self, NetInterface};
use crate::providers::LibreServer;
use crate::providers::librespeed::fetch_libre_servers;
use crate::providers::ookla::{self, OoklaServer};
use crate::system::process_stats;

const LIBRE_CACHE_TTL: Duration = Duration::from_secs(3600);

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/system/status", get(status))
        .route("/api/system/interfaces", get(interfaces))
        .route("/api/system/servers/librespeed", get(libre_servers))
        .route("/api/system/ookla/status", get(ookla_status))
        .route("/api/system/ookla/consent", post(ookla_consent))
        .route("/api/system/ookla/install", post(ookla_install))
        .route("/api/system/servers/ookla", get(ookla_servers))
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    let stats = process_stats();
    let version_info = s.version_checker.check().await;
    Json(json!({
        "version": version_info.current_version,
        "latest_version": version_info.latest_version,
        "update_available": version_info.update_available,
        "preview_mode": s.preview_mode,
        "preview_message": s.preview_message,
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

async fn ookla_status(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = config::load(&s.db).await?;
    let bin_path = s.ookla_bin_path();
    let installed = bin_path.is_file();
    let consented = cfg.ookla_license_agreed;
    let arch = std::env::consts::ARCH;
    Ok(Json(json!({
        "installed": installed,
        "consented": consented,
        "arch": arch,
    })))
}

async fn ookla_consent(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    config::set(&s.db, "ookla_license_agreed", "true")
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.ping_reload();
    Ok(Json(json!({
        "ok": true,
        "consented": true,
    })))
}

async fn ookla_install(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = config::load(&s.db).await?;
    if !cfg.ookla_license_agreed {
        return Err(ApiError::bad_request(
            "license agreement required before installation",
        ));
    }

    let download_url = match &s.ookla_download_url {
        Some(url) => url.clone(),
        None => {
            let arch = ookla::detect_arch()
                .map_err(|e| ApiError::bad_request(format!("{e}: {}", std::env::consts::ARCH)))?;
            ookla::official_download_url(arch)
                .ok_or_else(|| ApiError::bad_request(format!("unsupported architecture: {arch}")))?
                .to_string()
        }
    };

    let client = net::http_client(None);
    let bin_path = s.ookla_bin_path();
    ookla::download_and_install(&client, &download_url, &bin_path)
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("failed to download/install ookla cli: {e}"),
            )
        })?;

    Ok(Json(json!({
        "ok": true,
        "installed": true,
    })))
}

async fn ookla_servers(State(s): State<AppState>) -> ApiResult<Json<Vec<OoklaServer>>> {
    let bin_path = s.ookla_bin_path();
    if !bin_path.is_file() {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "ookla cli is not installed",
        ));
    }

    let servers = ookla::list_servers(&bin_path).await.map_err(|e| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("cannot list ookla servers: {e}"),
        )
    })?;

    Ok(Json(servers))
}
