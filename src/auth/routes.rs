use std::net::{IpAddr, SocketAddr};
use std::time::Instant;

use axum::extract::{ConnectInfo, Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::{
    AuthContext, SESSION_COOKIE_NAME, SESSION_DURATION_SECS, create_session, delete_all_sessions,
    delete_session, hash_password, verify_password_async,
};
use crate::api::{ApiError, ApiJson, ApiResult, AppState};
use crate::db::config::{self, PasswordMode};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/auth/status", get(get_status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(set_password))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthStatusResponse {
    pub mode: PasswordMode,
    pub authenticated: bool,
}

async fn get_status(
    State(s): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> ApiResult<Json<AuthStatusResponse>> {
    let cfg = config::load(&s.db).await?;
    Ok(Json(AuthStatusResponse {
        mode: cfg.password_mode,
        authenticated: auth.authenticated,
    }))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub password: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: i64,
}

/// `X-Forwarded-For` is client-controlled, so it only counts behind a trusted proxy.
fn client_ip(
    s: &AppState,
    headers: &HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
) -> IpAddr {
    let forwarded = s
        .trust_proxy
        .then(|| headers.get("x-forwarded-for")?.to_str().ok())
        .flatten()
        .and_then(|v| v.split(',').next()?.trim().parse().ok());
    forwarded
        .or(conn.map(|c| c.0.0.ip()))
        .unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    conn: Option<Extension<ConnectInfo<SocketAddr>>>,
    ApiJson(body): ApiJson<LoginRequest>,
) -> ApiResult<impl IntoResponse> {
    let ip = client_ip(&s, &headers, conn);
    if let Err(wait) = s.login_lockout.check(ip, Instant::now()) {
        let secs = wait.as_secs().max(1);
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, secs.to_string())],
            Json(serde_json::json!({ "error": "too many attempts" })),
        )
            .into_response());
    }
    let cfg = config::load(&s.db).await?;
    if cfg.password_mode != PasswordMode::None {
        if !verify_password_async(body.password.clone(), cfg.password_hash.clone()).await {
            s.login_lockout.fail(ip, Instant::now());
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid password"));
        }
        s.login_lockout.succeed(ip);
    }

    let (token, expires_at) = create_session(&s.db).await?;
    let cookie_val = format!(
        "{SESSION_COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={SESSION_DURATION_SECS}"
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        cookie_val.parse().map_err(|e| anyhow::anyhow!("{e}"))?,
    );

    Ok((headers, Json(LoginResponse { token, expires_at })).into_response())
}

async fn logout(
    State(s): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> ApiResult<impl IntoResponse> {
    if let Some(token) = auth.token {
        delete_session(&s.db, &token).await?;
    }

    let cookie_val = format!("{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        cookie_val.parse().map_err(|e| anyhow::anyhow!("{e}"))?,
    );

    Ok((headers, Json(serde_json::json!({ "ok": true }))))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PasswordRequest {
    #[serde(default, alias = "current_password")]
    pub current_password: Option<String>,
    #[serde(default, alias = "new_password", alias = "newPassword")]
    pub password: Option<String>,
    #[serde(default, alias = "password_mode", alias = "passwordMode")]
    pub mode: Option<PasswordMode>,
}

async fn set_password(
    State(s): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    ApiJson(body): ApiJson<PasswordRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let cfg = config::load(&s.db).await?;

    if cfg.password_mode != PasswordMode::None {
        let is_authed = auth.authenticated
            || match body.current_password.clone() {
                Some(p) => verify_password_async(p, cfg.password_hash.clone()).await,
                None => false,
            };

        if !is_authed {
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized"));
        }
    }

    // Determine removal vs update
    if body.mode == Some(PasswordMode::None)
        || (body.password.as_deref() == Some("") && body.mode.is_none())
    {
        // Remove password
        config::set_password_and_mode(&s.db, "", PasswordMode::None)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        delete_all_sessions(&s.db).await?;
        s.ping_reload();
        return Ok(Json(serde_json::json!({ "ok": true })));
    }

    if let Some(new_password) = body.password {
        if new_password.is_empty() {
            return Err(ApiError::bad_request("password cannot be empty"));
        }
        let hash = hash_password(&new_password)?;
        let mode = match body.mode {
            Some(m) if m != PasswordMode::None => m,
            _ if cfg.password_mode != PasswordMode::None => cfg.password_mode,
            _ => PasswordMode::Protected,
        };
        config::set_password_and_mode(&s.db, &hash, mode)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        delete_all_sessions(&s.db).await?;
        s.ping_reload();
        return Ok(Json(serde_json::json!({ "ok": true })));
    }

    if let Some(new_mode) = body.mode {
        if cfg.password_hash.is_empty() {
            return Err(ApiError::bad_request("cannot set mode without password"));
        }
        config::set_password_and_mode(&s.db, &cfg.password_hash, new_mode)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        s.ping_reload();
        return Ok(Json(serde_json::json!({ "ok": true })));
    }

    Err(ApiError::bad_request("no password or mode provided"))
}
