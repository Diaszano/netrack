pub mod lockout;
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

static ARGON_PERMITS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
const FAILED_AUTH_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

/// Argon2 on the blocking pool, at most 2 at once; failures wait to slow brute force.
pub async fn verify_password_async(password: String, hash: String) -> bool {
    let Ok(permit) = ARGON_PERMITS.acquire().await else {
        return false;
    };
    // Permit moves into the closure: held exactly while Argon2 runs, even if the caller is dropped.
    let ok = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        verify_password(&password, &hash)
    })
    .await
    .unwrap_or(false);
    if !ok {
        tokio::time::sleep(FAILED_AUTH_DELAY).await;
    }
    ok
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

pub async fn delete_expired_sessions(db: &Db, now_secs: i64) -> anyhow::Result<usize> {
    let n = db
        .call(move |conn| {
            Ok(conn.execute(
                "DELETE FROM sessions WHERE expires_at <= ?1",
                rusqlite::params![now_secs],
            )?)
        })
        .await?;
    Ok(n)
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

    let cfg = match config::load(&state.db).await {
        Ok(c) => c,
        Err(e) => return ApiError::from(e).into_response(),
    };

    let cookie_token = req
        .headers()
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .and_then(extract_token_from_cookie);
    let bearer_token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(extract_token_from_bearer);

    let mut authenticated = match &cookie_token {
        Some(t) => validate_session(&state.db, t).await.unwrap_or(false),
        None => false,
    };

    // Bearer is checked even when a cookie is present (older primaries send both).
    // Only a Bearer token may be the raw password; a stale cookie must not cost Argon2.
    if !authenticated && let Some(t) = &bearer_token {
        authenticated = validate_session(&state.db, t).await.unwrap_or(false);
        if !authenticated && cfg.password_mode != PasswordMode::None {
            authenticated = verify_password_async(t.clone(), cfg.password_hash.clone()).await;
        }
    }
    let token = cookie_token.or(bearer_token);

    req.extensions_mut().insert(AuthContext {
        authenticated,
        token,
    });

    let path = req.uri().path().to_owned();
    // Specific API route exemptions
    if path == "/api/auth/login" || path == "/api/auth/status" || path == "/api/prometheus/metrics"
    {
        return next.run(req).await;
    }

    match cfg.password_mode {
        PasswordMode::None => next.run(req).await,
        PasswordMode::Public => {
            let read_only = matches!(
                req.method(),
                &Method::GET | &Method::HEAD | &Method::OPTIONS
            );
            if (read_only && !requires_auth_in_public(&path)) || authenticated {
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

/// Secrets-bearing routes stay behind login even for GETs in public mode.
fn requires_auth_in_public(path: &str) -> bool {
    ["/api/integrations", "/api/nodes", "/api/data"]
        .iter()
        .any(|p| path.starts_with(p))
}
