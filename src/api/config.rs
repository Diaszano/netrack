use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::{Json, Router};
use serde::Deserialize;

use super::{ApiError, ApiResult, AppState};
use crate::db::config::{self, AppConfig, ConfigError};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/config", get(get_config))
        .route("/api/config/{key}", patch(patch_config))
}

async fn get_config(State(s): State<AppState>) -> ApiResult<Json<AppConfig>> {
    Ok(Json(config::load(&s.db).await?))
}

#[derive(Deserialize)]
struct Patch {
    value: String,
}

async fn patch_config(
    State(s): State<AppState>,
    Path(key): Path<String>,
    Json(body): Json<Patch>,
) -> ApiResult<StatusCode> {
    match config::set(&s.db, &key, &body.value).await {
        Ok(()) => {
            s.ping_reload();
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e @ (ConfigError::UnknownKey(_) | ConfigError::Invalid { .. })) => {
            Err(ApiError::bad_request(e.to_string()))
        }
        Err(ConfigError::Db(e)) => Err(e.into()),
    }
}
