use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

use crate::api::{ApiError, AppState};
use crate::auth::{AuthContext, verify_password};
use crate::db::config::{self, PasswordMode};
use crate::db::tests_repo::{self, SpeedtestRow};

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut table = [0xFFu8; 256];
    for (i, &b) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
        .iter()
        .enumerate()
    {
        table[b as usize] = i as u8;
    }
    let input = input.trim().as_bytes();
    if input.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::with_capacity((input.len() * 3) / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for &b in input {
        if b == b'=' {
            break;
        }
        let v = table[b as usize];
        if v == 0xFF {
            return None;
        }
        buf = (buf << 6) | (v as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

fn extract_basic_auth(auth_header: &str) -> Option<(String, String)> {
    let auth_header = auth_header.trim();
    let b64 = if let Some(rest) = auth_header.strip_prefix("Basic ") {
        rest.trim()
    } else if let Some(rest) = auth_header.strip_prefix("basic ") {
        rest.trim()
    } else {
        return None;
    };
    let bytes = decode_base64(b64)?;
    let decoded = String::from_utf8(bytes).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}

fn escape_label(val: &str) -> String {
    val.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn format_labels(row: &SpeedtestRow) -> String {
    let provider = escape_label(&row.provider);
    let server_id = escape_label(row.server_id.as_deref().unwrap_or(""));
    let server_name = escape_label(row.server_name.as_deref().unwrap_or(""));
    format!("provider=\"{provider}\",server_id=\"{server_id}\",server_name=\"{server_name}\"")
}

pub fn format_metrics(latest: Option<&SpeedtestRow>) -> String {
    let mut out = String::new();

    out.push_str("# HELP netrack_ping_latency_ms Ping latency in milliseconds\n");
    out.push_str("# TYPE netrack_ping_latency_ms gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.ping.unwrap_or(0.0);
        out.push_str(&format!("netrack_ping_latency_ms{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_jitter_ms Jitter in milliseconds\n");
    out.push_str("# TYPE netrack_jitter_ms gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.jitter.unwrap_or(0.0);
        out.push_str(&format!("netrack_jitter_ms{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_download_speed_mbps Download speed in megabits per second\n");
    out.push_str("# TYPE netrack_download_speed_mbps gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.download.unwrap_or(0.0);
        out.push_str(&format!("netrack_download_speed_mbps{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_upload_speed_mbps Upload speed in megabits per second\n");
    out.push_str("# TYPE netrack_upload_speed_mbps gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.upload.unwrap_or(0.0);
        out.push_str(&format!("netrack_upload_speed_mbps{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_test_duration_seconds Duration of the speed test in seconds\n");
    out.push_str("# TYPE netrack_test_duration_seconds gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.duration.unwrap_or(0.0);
        out.push_str(&format!("netrack_test_duration_seconds{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_last_test_timestamp_seconds Timestamp of the last speed test in seconds\n");
    out.push_str("# TYPE netrack_last_test_timestamp_seconds gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = row.created_at / 1000;
        out.push_str(&format!("netrack_last_test_timestamp_seconds{{{labels}}} {val}\n"));
    }

    out.push_str("# HELP netrack_test_success Whether the last speed test succeeded (1) or failed (0)\n");
    out.push_str("# TYPE netrack_test_success gauge\n");
    if let Some(row) = latest {
        let labels = format_labels(row);
        let val = if row.error.is_none() { 1 } else { 0 };
        out.push_str(&format!("netrack_test_success{{{labels}}} {val}\n"));
    }

    out
}

pub async fn metrics_handler(State(state): State<AppState>, req: Request) -> Response {
    let cfg = match config::load(&state.db).await {
        Ok(c) => c,
        Err(e) => return ApiError::from(e).into_response(),
    };

    if cfg.password_mode != PasswordMode::None {
        let auth_ctx = req.extensions().get::<AuthContext>();
        let session_valid = auth_ctx.map_or(false, |c| c.authenticated);

        let basic_valid = if !session_valid {
            req.headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(extract_basic_auth)
                .map_or(false, |(_user, pass)| {
                    verify_password(&pass, &cfg.password_hash)
                })
        } else {
            false
        };

        if !session_valid && !basic_valid {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(header::WWW_AUTHENTICATE, "Basic realm=\"netrack\"")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"error":"unauthorized"}"#))
                .unwrap();
        }
    }

    let latest = match tests_repo::list(&state.db, None, 1).await {
        Ok(list) => list.into_iter().next(),
        Err(e) => return ApiError::from(e).into_response(),
    };

    let body = format_metrics(latest.as_ref());
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )
        .body(Body::from(body))
        .unwrap()
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/prometheus/metrics", get(metrics_handler))
}
