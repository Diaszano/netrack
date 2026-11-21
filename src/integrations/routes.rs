use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Integration, IntegrationEvent, MASK, SECRET_KEYS, SUPPORTED_CHANNELS, SUPPORTED_EVENTS,
    channels, masked, unmask,
};
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
    Ok(Json(items.into_iter().map(mask_integration).collect()))
}

fn mask_integration(mut item: Integration) -> Integration {
    item.config = masked(&item.config);
    item
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
    // On create there is no stored value to restore, so "***" would be saved as the secret.
    if let Some(k) = SECRET_KEYS
        .iter()
        .find(|k| body.config.get(**k).and_then(Value::as_str) == Some(MASK))
    {
        return Err(ApiError::bad_request(format!(
            "'{k}' cannot be the mask value '{MASK}'"
        )));
    }

    let created = super::insert(
        &s.db,
        body.name.trim().to_string(),
        body.channel_type,
        body.config,
        body.events,
        body.enabled,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(mask_integration(created))))
}

async fn update_integration(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
    ApiJson(body): ApiJson<IntegrationPayload>,
) -> ApiResult<Json<Integration>> {
    validate_payload(&body)?;

    let Some(stored) = super::get(&s.db, id).await? else {
        return Err(ApiError::new(
            StatusCode::NOT_FOUND,
            "integration not found",
        ));
    };

    let updated = super::update(
        &s.db,
        id,
        body.name.trim().to_string(),
        body.channel_type,
        unmask(body.config, &stored.config),
        body.events,
        body.enabled,
    )
    .await?;

    match updated {
        Some(item) => Ok(Json(mask_integration(item))),
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

    let event = IntegrationEvent::Test {
        timestamp: chrono::Utc::now().timestamp_millis(),
    };

    // Channels build their own requests, so the 10s cap wraps the whole dispatch.
    let sent = tokio::time::timeout(
        Duration::from_secs(10),
        channels::dispatch(
            &integration.channel_type,
            &s.http,
            &integration.config,
            &event,
        ),
    )
    .await
    .unwrap_or_else(|_| Err(anyhow::anyhow!("timed out after 10s")));
    match sent {
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
