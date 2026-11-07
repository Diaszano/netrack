//! JSON HTTP API. [`router`] is the single router builder; later plans merge
//! their routes into it.

mod config;
mod speedtests;
mod system;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use tokio::sync::watch;

use crate::db::Db;
use crate::providers::LibreServer;
use crate::providers::librespeed::DEFAULT_LIST_URL;
use crate::runner::Runner;
use crate::scheduler::Scheduler;

pub(crate) type LibreCache = Arc<Mutex<Option<(Instant, Vec<LibreServer>)>>>;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub runner: Runner,
    /// Pinged after every config write. Must live as long as the app: the
    /// scheduler stops when the sender is dropped.
    pub reload: watch::Sender<()>,
    pub scheduler: Arc<Scheduler>,
    pub started_at: Instant,
    /// Flips to true on shutdown; ends long-lived streams (SSE).
    pub shutdown: watch::Sender<bool>,
    pub libre_list_url: String,
    pub(crate) libre_cache: LibreCache,
}

impl AppState {
    pub fn new(
        db: Db,
        runner: Runner,
        reload: watch::Sender<()>,
        scheduler: Arc<Scheduler>,
    ) -> AppState {
        AppState {
            db,
            runner,
            reload,
            scheduler,
            started_at: Instant::now(),
            shutdown: watch::channel(false).0,
            libre_list_url: DEFAULT_LIST_URL.to_owned(),
            libre_cache: Arc::default(),
        }
    }

    pub fn trigger_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Tells the scheduler to re-read the config.
    pub(crate) fn ping_reload(&self) {
        self.reload.send_replace(());
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(speedtests::routes())
        .merge(config::routes())
        .merge(system::routes())
        .merge(crate::auth::routes::routes())
        .fallback(crate::web::serve)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::middleware,
        ))
        .with_state(state)
}

/// An error rendered as `{ "error": "<message>" }`.
pub(crate) struct ApiError(StatusCode, String);

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> ApiError {
        ApiError(status, message.into())
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> ApiError {
        tracing::error!(error = %e, "internal error in api handler");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

async fn method_not_allowed() -> ApiError {
    ApiError::new(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
}

/// `Json` extractor whose rejections render as `{ "error": ... }`.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub(crate) struct ApiJson<T>(pub T);

/// `Path` extractor whose rejections render as `{ "error": ... }`.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub(crate) struct ApiPath<T>(pub T);

/// `Query` extractor whose rejections render as `{ "error": ... }`.
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(ApiError))]
pub(crate) struct ApiQuery<T>(pub T);

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> ApiError {
        ApiError::new(r.status(), r.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(r: PathRejection) -> ApiError {
        ApiError::new(r.status(), r.body_text())
    }
}

impl From<QueryRejection> for ApiError {
    fn from(r: QueryRejection) -> ApiError {
        ApiError::new(r.status(), r.body_text())
    }
}
