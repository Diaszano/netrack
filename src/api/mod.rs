//! JSON HTTP API. [`router`] is the single router builder; later plans merge
//! their routes into it.

mod config;
mod speedtests;
mod system;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Json;
use axum::Router;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::{StatusCode, header};
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
    pub data_dir: PathBuf,
    pub ookla_download_url: Option<String>,
    pub preview_mode: bool,
    pub preview_message: Option<String>,
    pub version_checker: Arc<crate::version::VersionChecker>,
    /// Shared outbound client. No total timeout: the node proxy carries SSE.
    pub http: reqwest::Client,
    pub login_lockout: Arc<crate::auth::lockout::Lockout>,
    /// Honor `X-Forwarded-For` for the client IP (only safe behind a reverse proxy).
    pub trust_proxy: bool,
    /// Add `Secure` to the session cookie (needs HTTPS in front).
    pub secure_cookie: bool,
    /// (password hash, password) last verified via Bearer: a primary node resends the raw
    /// password on every proxied request, which must not cost Argon2 or lockout attempts.
    pub(crate) bearer_cache: Arc<Mutex<Option<(String, String)>>>,
}

impl AppState {
    pub fn new(
        db: Db,
        runner: Runner,
        reload: watch::Sender<()>,
        scheduler: Arc<Scheduler>,
    ) -> AppState {
        let data_dir = std::env::var("NETRACK_DATA_DIR")
            .map_or_else(|_| PathBuf::from("/data"), PathBuf::from);
        AppState {
            db,
            runner,
            reload,
            scheduler,
            started_at: Instant::now(),
            shutdown: watch::channel(false).0,
            libre_list_url: DEFAULT_LIST_URL.to_owned(),
            libre_cache: Arc::default(),
            data_dir,
            ookla_download_url: None,
            preview_mode: false,
            preview_message: None,
            version_checker: Arc::new(crate::version::VersionChecker::default()),
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(5))
                .build()
                .expect("static reqwest client configuration is valid"),
            login_lockout: Arc::default(),
            trust_proxy: false,
            secure_cookie: false,
            bearer_cache: Arc::default(),
        }
    }

    pub fn with_data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.data_dir = path.into();
        self
    }

    pub fn with_ookla_download_url(mut self, url: impl Into<String>) -> Self {
        self.ookla_download_url = Some(url.into());
        self
    }

    pub fn with_preview(mut self, enabled: bool, message: Option<String>) -> Self {
        self.preview_mode = enabled;
        self.preview_message = if enabled && message.is_none() {
            Some(crate::settings::DEFAULT_PREVIEW_MESSAGE.to_string())
        } else {
            message
        };
        self
    }

    pub fn with_trust_proxy(mut self, trust: bool) -> Self {
        self.trust_proxy = trust;
        self
    }

    pub fn with_secure_cookie(mut self, secure: bool) -> Self {
        self.secure_cookie = secure;
        self
    }

    pub fn with_release_url(mut self, url: impl Into<String>) -> Self {
        self.version_checker =
            Arc::new(self.version_checker.as_ref().clone().with_release_url(url));
        self
    }

    pub fn with_version_checker(mut self, checker: Arc<crate::version::VersionChecker>) -> Self {
        self.version_checker = checker;
        self
    }

    pub fn ookla_bin_path(&self) -> PathBuf {
        self.data_dir.join("bin").join("speedtest")
    }

    pub fn trigger_shutdown(&self) {
        self.shutdown.send_replace(true);
    }

    /// Tells the scheduler to re-read the config.
    pub(crate) fn ping_reload(&self) {
        self.reload.send_replace(());
    }
}

pub async fn preview_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if state.preview_mode && req.uri().path().starts_with("/api/") {
        let method = req.method();
        let path = req.uri().path();
        let is_read_only = matches!(
            *method,
            axum::http::Method::GET | axum::http::Method::HEAD | axum::http::Method::OPTIONS
        );
        let is_auth_allowed = path == "/api/auth/login" || path == "/api/auth/logout";
        if !is_read_only && !is_auth_allowed {
            let msg = state
                .preview_message
                .as_deref()
                .unwrap_or(crate::settings::DEFAULT_PREVIEW_MESSAGE);
            return (StatusCode::FORBIDDEN, Json(json!({ "error": msg }))).into_response();
        }
    }
    next.run(req).await
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(speedtests::routes())
        .merge(config::routes())
        .merge(system::routes())
        .merge(crate::auth::routes::routes())
        .merge(crate::integrations::routes::routes())
        .merge(crate::prometheus::routes())
        .merge(crate::data_transfer::routes::routes())
        .merge(crate::nodes::routes::routes())
        .merge(crate::opengraph::routes())
        .fallback(crate::web::serve)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::auth::middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            preview_middleware,
        ))
        .with_state(state)
}

/// An error rendered as `{ "error": "<message>" }`.
pub(crate) struct ApiError(StatusCode, String, Option<u64>);

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> ApiError {
        ApiError(status, message.into(), None)
    }

    /// 429 with `Retry-After` (seconds, rounded up).
    pub(crate) fn too_many_attempts(wait: std::time::Duration) -> ApiError {
        let secs = wait.as_secs() + u64::from(wait.subsec_nanos() > 0);
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts".into(),
            Some(secs),
        )
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
        let mut res = (self.0, Json(json!({ "error": self.1 }))).into_response();
        if let Some(secs) = self.2 {
            res.headers_mut().insert(header::RETRY_AFTER, secs.into());
        }
        res
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
