//! Reusable in-process HTTP mock for measurement and provider tests.
//!
//! `spawn_mock()` / `spawn_mock_with(opts)` bind 127.0.0.1:0 and serve:
//! - `GET /down?bytes=N` — streams N zero bytes from a static 1 MiB buffer.
//! - `POST /up` — drains and discards the request body.
//! - `GET /empty` — empty 200 response.
//!
//! New routes go in [`router`]; new knobs go in [`MockOptions`].
#![allow(dead_code)]

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bytes::Bytes;
use futures::{StreamExt, stream};
use serde::Deserialize;
use tokio::task::JoinHandle;

const CHUNK: usize = 1024 * 1024;
static ZEROS: [u8; CHUNK] = [0; CHUNK];

/// Behaviour knobs for the mock server.
#[derive(Debug, Clone, Default)]
pub struct MockOptions {
    /// After this many bytes of a `/down` response, stop sending (but keep the
    /// connection open) to simulate a stalled server.
    pub stall_after_bytes: Option<u64>,
    /// Every route answers with this status and no body.
    pub fail_status: Option<u16>,
}

/// A running mock server; aborted when dropped.
pub struct MockServer {
    pub base_url: String,
    handle: JoinHandle<()>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

pub async fn spawn_mock() -> MockServer {
    spawn_mock_with(MockOptions::default()).await
}

pub async fn spawn_mock_with(opts: MockOptions) -> MockServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock listener");
    let addr = listener.local_addr().expect("mock local addr");
    let app = router(Arc::new(opts));
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("mock server");
    });
    MockServer {
        base_url: format!("http://{addr}"),
        handle,
    }
}

type Opts = Arc<MockOptions>;

fn router(opts: Opts) -> Router {
    Router::new()
        .route("/down", get(down))
        .route("/up", post(up))
        .route("/empty", get(empty))
        .with_state(opts)
}

fn failure(opts: &MockOptions) -> Option<Response> {
    opts.fail_status.map(|code| {
        StatusCode::from_u16(code)
            .expect("valid fail_status")
            .into_response()
    })
}

#[derive(Deserialize)]
struct DownQuery {
    bytes: u64,
}

async fn down(State(opts): State<Opts>, Query(q): Query<DownQuery>) -> Response {
    if let Some(resp) = failure(&opts) {
        return resp;
    }
    let sendable = opts.stall_after_bytes.map_or(q.bytes, |s| s.min(q.bytes));
    let stalls = sendable < q.bytes;
    let body = stream::unfold(sendable, |left| async move {
        if left == 0 {
            return None;
        }
        let n = left.min(CHUNK as u64) as usize;
        Some((
            Ok::<_, std::io::Error>(Bytes::from_static(&ZEROS[..n])),
            left - n as u64,
        ))
    });
    if stalls {
        Body::from_stream(body.chain(stream::pending())).into_response()
    } else {
        Body::from_stream(body).into_response()
    }
}

async fn up(State(opts): State<Opts>, body: Body) -> Response {
    if let Some(resp) = failure(&opts) {
        return resp;
    }
    let mut data = body.into_data_stream();
    while let Some(chunk) = data.next().await {
        if chunk.is_err() {
            return StatusCode::BAD_REQUEST.into_response();
        }
    }
    StatusCode::OK.into_response()
}

async fn empty(State(opts): State<Opts>) -> Response {
    if let Some(resp) = failure(&opts) {
        return resp;
    }
    StatusCode::OK.into_response()
}
