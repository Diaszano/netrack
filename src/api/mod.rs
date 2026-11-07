//! JSON HTTP API. [`router`] is the single router builder; later plans merge
//! their routes into it.

mod config;
mod speedtests;
mod system;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::Router;
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
            libre_list_url: DEFAULT_LIST_URL.to_owned(),
            libre_cache: Arc::default(),
        }
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
