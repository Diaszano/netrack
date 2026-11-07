//! Reusable in-process HTTP mock for measurement and provider tests.
//!
//! `spawn_mock()` / `spawn_mock_with(opts)` bind 127.0.0.1:0 and serve:
//! - `GET /down?bytes=N` — streams N zero bytes from a static 1 MiB buffer.
//! - `POST /up` — drains and discards the request body.
//! - `GET /empty` — empty 200 response.
//!
//! Cloudflare-shaped: `GET /__down?bytes=N` (for N=0 the response is delayed
//! by [`CF_SERVER_DELAY_MS`] and reports it in `server-timing`), `POST /__up`
//! and `GET /cdn-cgi/trace` (`colo=POA`).
//! LibreSpeed-shaped: `GET /backend/garbage.php?ckSize=N` (N MiB),
//! `GET|POST /backend/empty.php`, and `GET /servers.json` (public list shape).
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
/// Simulated server-side processing time of a Cloudflare `bytes=0` request.
pub const CF_SERVER_DELAY_MS: u64 = 80;
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
        .route("/__down", get(cf_down))
        .route("/__up", post(up))
        .route("/cdn-cgi/trace", get(cf_trace))
        .route("/backend/garbage.php", get(garbage))
        .route("/backend/empty.php", get(empty).post(up))
        .route("/servers.json", get(servers_json))
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

async fn cf_down(State(opts): State<Opts>, Query(q): Query<DownQuery>) -> Response {
    let probe = q.bytes == 0;
    if probe {
        tokio::time::sleep(std::time::Duration::from_millis(CF_SERVER_DELAY_MS)).await;
    }
    let mut resp = down(State(opts), Query(q)).await;
    if probe {
        resp.headers_mut().insert(
            "server-timing",
            format!("cfSpeedWorker;dur={CF_SERVER_DELAY_MS}")
                .parse()
                .expect("header value"),
        );
    }
    resp
}

async fn cf_trace(State(opts): State<Opts>) -> Response {
    if let Some(resp) = failure(&opts) {
        return resp;
    }
    "fl=1\nh=x\ncolo=POA\n".into_response()
}

#[derive(Deserialize)]
struct GarbageQuery {
    #[serde(rename = "ckSize")]
    ck_size: u64,
}

async fn garbage(State(opts): State<Opts>, Query(q): Query<GarbageQuery>) -> Response {
    down(
        State(opts),
        Query(DownQuery {
            bytes: q.ck_size * CHUNK as u64,
        }),
    )
    .await
}

/// The public LibreSpeed list shape, including escaped slashes.
async fn servers_json() -> Response {
    let body = r#"[
{"id":1,"name":"Alpha, Brazil","server":"\/\/alpha.example.com\/","dlURL":"garbage.php","ulURL":"empty.php","pingURL":"empty.php","getIpURL":"getIP.php","sponsorName":"A","sponsorURL":"https:\/\/a.example","pingURL2":"x"},
{"id":2,"name":"Beta, Chile","server":"https:\/\/beta.example.com\/","dlURL":"garbage.php","ulURL":"empty.php","pingURL":"empty.php","getIpURL":"getIP.php"}
]"#;
    ([("content-type", "application/json")], body).into_response()
}
