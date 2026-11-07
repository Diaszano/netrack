pub mod routes;

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use crate::api::{ApiError, AppState};
use crate::db::Db;
use crate::db::config::{self, PasswordMode};

pub const SESSION_COOKIE_NAME: &str = "netrack_session";
pub const SESSION_DURATION_SECS: i64 = 30 * 24 * 3600; // 30 days

#[derive(Clone, Debug, Default)]
pub struct AuthContext {
    pub authenticated: bool,
    pub token: Option<String>,
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt_bytes: [u8; 16] = rand::random();
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| anyhow::anyhow!("failed to encode salt: {e}"))?;
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if hash.is_empty() {
        return false;
    }
    let parsed_hash = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

pub fn generate_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub async fn create_session(db: &Db) -> anyhow::Result<(String, i64)> {
    let token = generate_token();
    let now = chrono::Utc::now().timestamp();
    let expires_at = now + SESSION_DURATION_SECS;
    let t = token.clone();
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO sessions (token, created_at, expires_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![t, now, expires_at],
        )?;
        Ok(())
    })
    .await?;
    Ok((token, expires_at))
}

pub async fn validate_session(db: &Db, token: &str) -> anyhow::Result<bool> {
    let t = token.to_owned();
    let now = chrono::Utc::now().timestamp();
    let valid = db
        .call(move |conn| {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE token = ?1 AND expires_at > ?2",
                rusqlite::params![t, now],
                |r| r.get(0),
            )?;
            Ok(count > 0)
        })
        .await?;
    Ok(valid)
}

pub async fn delete_session(db: &Db, token: &str) -> anyhow::Result<()> {
    let t = token.to_owned();
    db.call(move |conn| {
        conn.execute(
            "DELETE FROM sessions WHERE token = ?1",
            rusqlite::params![t],
        )?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn delete_all_sessions(db: &Db) -> anyhow::Result<()> {
    db.call(|conn| {
        conn.execute("DELETE FROM sessions", [])?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub fn extract_token_from_cookie(cookie_header: &str) -> Option<String> {
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if let Some(val) = pair.strip_prefix("netrack_session=") {
            let val = val.trim();
            if !val.is_empty() {
                return Some(val.to_owned());
            }
        }
    }
    None
}

pub fn extract_token_from_bearer(auth_header: &str) -> Option<String> {
    let auth_header = auth_header.trim();
    if let Some(token) = auth_header.strip_prefix("Bearer ") {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }
    None
}

pub async fn middleware(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    if !req.uri().path().starts_with("/api/") {
        return next.run(req).await;
    }

    let mut token = req
        .headers()
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(extract_token_from_cookie);

    if token.is_none() {
        token = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(extract_token_from_bearer);
    }

    let mut authenticated = match &token {
        Some(t) => validate_session(&state.db, t).await.unwrap_or(false),
        None => false,
    };

    if !authenticated
        && let Some(t) = &token
        && let Ok(cfg) = config::load(&state.db).await
        && cfg.password_mode != PasswordMode::None
        && verify_password(t, &cfg.password_hash)
    {
        authenticated = true;
    }

    req.extensions_mut().insert(AuthContext {
        authenticated,
        token,
    });

    let path = req.uri().path();
    // Specific API route exemptions
    if path == "/api/auth/login" || path == "/api/auth/status" || path == "/api/prometheus/metrics"
    {
        return next.run(req).await;
    }

    let cfg = match config::load(&state.db).await {
        Ok(c) => c,
        Err(e) => return ApiError::from(e).into_response(),
    };
    match cfg.password_mode {
        PasswordMode::None => next.run(req).await,
        PasswordMode::Public => {
            if matches!(
                req.method(),
                &Method::GET | &Method::HEAD | &Method::OPTIONS
            ) || authenticated
            {
                next.run(req).await
            } else {
                ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized").into_response()
            }
        }
        PasswordMode::Protected => {
            if authenticated {
                next.run(req).await
            } else {
                ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized").into_response()
            }
        }
    }
}
