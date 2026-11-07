use axum::Router;
use axum::extract::State;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use chrono::{TimeZone, Utc};

use crate::api::AppState;
use crate::db::tests_repo;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/opengraph/image", get(opengraph_image))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub async fn opengraph_image(State(state): State<AppState>) -> Response {
    let latest_test = tests_repo::last_successful(&state.db, 1)
        .await
        .ok()
        .and_then(|mut list| list.pop());

    let (dl, ul, ping, footer, is_empty) = match latest_test {
        Some(t) => {
            let dl = t
                .download
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "--".to_string());
            let ul = t
                .upload
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "--".to_string());
            let ping = t
                .ping
                .map(|v| format!("{:.1}", v))
                .unwrap_or_else(|| "--".to_string());

            let mut info_parts = Vec::new();
            if let Some(server) = &t.server_name {
                info_parts.push(server.clone());
            } else if !t.provider.is_empty() {
                info_parts.push(t.provider.to_uppercase());
            }

            if let Some(utc_dt) = Utc.timestamp_millis_opt(t.created_at).single() {
                let local_dt = utc_dt.with_timezone(&state.db.tz());
                info_parts.push(local_dt.format("%Y-%m-%d %H:%M %Z").to_string());
            }

            let footer_text = if info_parts.is_empty() {
                "Latest speedtest result".to_string()
            } else {
                info_parts.join("  •  ")
            };

            (dl, ul, ping, footer_text, false)
        }
        None => (
            "--".to_string(),
            "--".to_string(),
            "--".to_string(),
            "No speed tests recorded yet".to_string(),
            true,
        ),
    };

    let dl_escaped = escape_xml(&dl);
    let ul_escaped = escape_xml(&ul);
    let ping_escaped = escape_xml(&ping);
    let footer_escaped = escape_xml(&footer);

    let empty_notice = if is_empty {
        r##"<text x="600" y="470" text-anchor="middle" font-family="system-ui, -apple-system, sans-serif" font-size="20" fill="#64748b" letter-spacing="1">Awaiting first test run</text>"##
    } else {
        ""
    };

    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 630" width="1200" height="630">
  <defs>
    <linearGradient id="bg" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#0b0f19" />
      <stop offset="100%" stop-color="#111827" />
    </linearGradient>
    <linearGradient id="brandGrad" x1="0%" y1="0%" x2="100%" y2="0%">
      <stop offset="0%" stop-color="#38bdf8" />
      <stop offset="100%" stop-color="#818cf8" />
    </linearGradient>
    <filter id="shadow" x="-5%" y="-5%" width="110%" height="115%">
      <feDropShadow dx="0" dy="8" stdDeviation="12" flood-color="#000000" flood-opacity="0.4" />
    </filter>
  </defs>

  <!-- Background -->
  <rect width="1200" height="630" fill="url(#bg)" />

  <!-- Subtle Accent border -->
  <rect x="20" y="20" width="1160" height="590" rx="24" fill="none" stroke="#1e293b" stroke-width="2" />

  <!-- Brand / Header -->
  <g transform="translate(80, 80)">
    <!-- Brand Mark (Speed Gauge Icon) -->
    <circle cx="28" cy="28" r="26" fill="#1e293b" stroke="#38bdf8" stroke-width="3" />
    <path d="M 28 28 L 42 16" stroke="#38bdf8" stroke-width="3.5" stroke-linecap="round" />
    <circle cx="28" cy="28" r="4" fill="#38bdf8" />
    
    <!-- Title -->
    <text x="70" y="32" font-family="system-ui, -apple-system, sans-serif" font-size="34" font-weight="800" fill="#f8fafc" letter-spacing="4">NETRACK</text>
    <text x="70" y="54" font-family="system-ui, -apple-system, sans-serif" font-size="14" font-weight="600" fill="#94a3b8" letter-spacing="2">NETWORK SPEED MONITOR</text>
  </g>

  <!-- Metrics Grid -->
  <g transform="translate(80, 190)">
    <!-- Download Card -->
    <g transform="translate(0, 0)">
      <rect width="320" height="230" rx="18" fill="#131c2e" stroke="#1e293b" stroke-width="2" filter="url(#shadow)" />
      <text x="32" y="52" font-family="system-ui, -apple-system, sans-serif" font-size="15" font-weight="700" fill="#38bdf8" letter-spacing="2">DOWNLOAD</text>
      <text x="32" y="140" font-family="system-ui, -apple-system, sans-serif" font-size="64" font-weight="800" fill="#f8fafc">{dl_escaped}</text>
      <text x="32" y="185" font-family="system-ui, -apple-system, sans-serif" font-size="20" font-weight="600" fill="#64748b">Mbps</text>
    </g>

    <!-- Upload Card -->
    <g transform="translate(360, 0)">
      <rect width="320" height="230" rx="18" fill="#131c2e" stroke="#1e293b" stroke-width="2" filter="url(#shadow)" />
      <text x="32" y="52" font-family="system-ui, -apple-system, sans-serif" font-size="15" font-weight="700" fill="#818cf8" letter-spacing="2">UPLOAD</text>
      <text x="32" y="140" font-family="system-ui, -apple-system, sans-serif" font-size="64" font-weight="800" fill="#f8fafc">{ul_escaped}</text>
      <text x="32" y="185" font-family="system-ui, -apple-system, sans-serif" font-size="20" font-weight="600" fill="#64748b">Mbps</text>
    </g>

    <!-- Ping Card -->
    <g transform="translate(720, 0)">
      <rect width="320" height="230" rx="18" fill="#131c2e" stroke="#1e293b" stroke-width="2" filter="url(#shadow)" />
      <text x="32" y="52" font-family="system-ui, -apple-system, sans-serif" font-size="15" font-weight="700" fill="#fbbf24" letter-spacing="2">PING</text>
      <text x="32" y="140" font-family="system-ui, -apple-system, sans-serif" font-size="64" font-weight="800" fill="#f8fafc">{ping_escaped}</text>
      <text x="32" y="185" font-family="system-ui, -apple-system, sans-serif" font-size="20" font-weight="600" fill="#64748b">ms</text>
    </g>
  </g>

  <!-- Empty Notice (if applicable) -->
  {empty_notice}

  <!-- Footer / Status Info -->
  <g transform="translate(80, 510)">
    <rect width="1040" height="48" rx="12" fill="#0f172a" stroke="#1e293b" stroke-width="1.5" />
    <circle cx="28" cy="24" r="5" fill="{status_dot_color}" />
    <text x="48" y="30" font-family="system-ui, -apple-system, sans-serif" font-size="16" font-weight="500" fill="#94a3b8">{footer_escaped}</text>
  </g>
</svg>"##,
        dl_escaped = dl_escaped,
        ul_escaped = ul_escaped,
        ping_escaped = ping_escaped,
        empty_notice = empty_notice,
        status_dot_color = if is_empty { "#64748b" } else { "#10b981" },
        footer_escaped = footer_escaped,
    );

    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "no-cache, max-age=60"),
        ],
        svg,
    )
        .into_response()
}
