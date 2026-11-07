use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Integration, IntegrationEvent, SUPPORTED_CHANNELS, SUPPORTED_EVENTS, channels};
use crate::api::{ApiError, ApiJson, ApiPath, ApiResult, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/integrations", get(list_integrations))
        .route("/api/integrations", post(create_integration))
        .route("/api/integrations/{id}", put(update_integration))
        .route("/api/integrations/{id}", delete(delete_integration))
        .route("/api/integrations/{id}/test", post(test_integration))
}

async fn list_integrations(State(s): State<AppState>) -> ApiResult<Json<Vec<Integration>>> {
    let items = super::list(&s.db).await?;
    Ok(Json(items))
}

#[derive(Debug, Deserialize)]
pub struct IntegrationPayload {
    pub name: String,
    pub channel_type: String,
    pub config: Value,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

fn validate_payload(payload: &IntegrationPayload) -> Result<(), ApiError> {
    let name = payload.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name cannot be empty"));
    }

    if !SUPPORTED_CHANNELS.contains(&payload.channel_type.as_str()) {
        return Err(ApiError::bad_request(format!(
            "unsupported channel_type '{}'. Supported: {}",
            payload.channel_type,
            SUPPORTED_CHANNELS.join(", ")
        )));
    }

    if !payload.config.is_object() {
        return Err(ApiError::bad_request("config must be a JSON object"));
    }

    for ev in &payload.events {
        if !SUPPORTED_EVENTS.contains(&ev.as_str()) {
            return Err(ApiError::bad_request(format!(
                "unsupported event '{ev}'. Supported: {}",
                SUPPORTED_EVENTS.join(", ")
            )));
        }
    }

    Ok(())
}

async fn create_integration(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<IntegrationPayload>,
) -> ApiResult<(StatusCode, Json<Integration>)> {
    validate_payload(&body)?;

    let created = super::insert(
        &s.db,
        body.name.trim().to_string(),
        body.channel_type,
        body.config,
        body.events,
        body.enabled,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(created)))
}

async fn update_integration(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
    ApiJson(body): ApiJson<IntegrationPayload>,
) -> ApiResult<Json<Integration>> {
    validate_payload(&body)?;

    let updated = super::update(
        &s.db,
        id,
        body.name.trim().to_string(),
        body.channel_type,
        body.config,
        body.events,
        body.enabled,
    )
    .await?;

    match updated {
        Some(item) => Ok(Json(item)),
        None => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "integration not found",
        )),
    }
}

async fn delete_integration(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<StatusCode> {
    let deleted = super::delete(&s.db, id).await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "integration not found",
        ))
    }
}

async fn test_integration(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<Json<Value>> {
    let integration = match super::get(&s.db, id).await? {
        Some(item) => item,
        None => {
            return Err(ApiError::new(
                StatusCode::NOT_FOUND,
                "integration not found",
            ));
        }
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let event = IntegrationEvent::Test {
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    match channels::dispatch(
        &integration.channel_type,
        &client,
        &integration.config,
        &event,
    )
    .await
    {
        Ok(()) => Ok(Json(json!({
            "success": true,
            "message": "Notification sent successfully"
        }))),
        Err(e) => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("notification delivery failed: {e}"),
        )),
    }
}
