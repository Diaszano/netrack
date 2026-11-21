//! Embedded static asset pipeline and SPA fallback handler.

use axum::body::Body;
use axum::http::{Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

use crate::api::ApiError;

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

// style-src needs 'unsafe-inline' while views still use style="" attributes.
const CSP: &str = "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'";

/// Look up an asset by its absolute URL path (e.g. `/styles.css`).
pub fn get_asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    ASSETS
        .iter()
        .find(|(url, _, _)| *url == path)
        .map(|(_, ct, bytes)| (*ct, *bytes))
}

pub(crate) async fn serve(uri: Uri, method: Method) -> Response {
    let path = uri.path();

    // 1. Any path starting with /api/ or equal to /api returns 404 JSON error
    if path == "/api" || path.starts_with("/api/") {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    }

    // 2. Only GET and HEAD are allowed; other methods return 405 JSON error
    if method != Method::GET && method != Method::HEAD {
        return ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method not allowed").into_response();
    }

    // 3. Exact match in ASSETS
    if let Some((content_type, bytes)) = get_asset(path) {
        let cache_control = if path.starts_with("/fonts/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };

        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(bytes)
        };

        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, cache_control)
            .header(header::CONTENT_SECURITY_POLICY, CSP)
            .header(header::X_FRAME_OPTIONS, "DENY")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    // 4. Any other path containing a '.' in its last segment returns 404 JSON error
    let last_segment = path.rsplit('/').next().unwrap_or("");
    if last_segment.contains('.') {
        return ApiError::new(StatusCode::NOT_FOUND, "not found").into_response();
    }

    // 5. SPA paths without an extension return index.html
    if let Some((_, bytes)) = get_asset("/index.html") {
        let body = if method == Method::HEAD {
            Body::empty()
        } else {
            Body::from(bytes)
        };

        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-cache")
            .header(header::CONTENT_SECURITY_POLICY, CSP)
            .header(header::X_FRAME_OPTIONS, "DENY")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .body(body)
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
    }

    ApiError::new(StatusCode::NOT_FOUND, "not found").into_response()
}
