use std::convert::Infallible;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{Datelike, NaiveDate};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use super::{ApiError, ApiPath, ApiQuery, ApiResult, AppState};
use crate::db::config;
use crate::db::stats::{self, Statistics};
use crate::db::tests_repo::{self, SpeedtestRow, TestKind};
use crate::runner::RunEvent;

const DEFAULT_LIMIT: u32 = 30;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/speedtests", get(list))
        .route("/api/speedtests/run", post(run))
        .route("/api/speedtests/status", get(status))
        .route("/api/speedtests/pause", post(pause))
        .route("/api/speedtests/continue", post(resume))
        .route("/api/speedtests/statistics", get(statistics))
        .route("/api/speedtests/events", get(events))
        .route("/api/speedtests/{id}", get(get_one).delete(delete_one))
}

#[derive(Deserialize)]
struct ListQuery {
    before: Option<i64>,
    limit: Option<u32>,
}

async fn list(
    State(s): State<AppState>,
    ApiQuery(q): ApiQuery<ListQuery>,
) -> ApiResult<Json<Vec<SpeedtestRow>>> {
    let rows = tests_repo::list(&s.db, q.before, q.limit.unwrap_or(DEFAULT_LIMIT)).await?;
    Ok(Json(rows))
}

async fn get_one(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<Json<SpeedtestRow>> {
    tests_repo::get(&s.db, id)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "speedtest not found"))
}

async fn delete_one(State(s): State<AppState>, ApiPath(id): ApiPath<i64>) -> ApiResult<StatusCode> {
    if tests_repo::delete(&s.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "speedtest not found"))
    }
}

async fn run(State(s): State<AppState>) -> ApiResult<(StatusCode, Json<Value>)> {
    // Acquired here, not in the task, so concurrent requests cannot both get 202.
    let guard = s
        .runner
        .try_acquire()
        .map_err(|_| ApiError::new(StatusCode::CONFLICT, "test already running"))?;
    // Detached on purpose: awaiting the run in the request would let a client
    // disconnect drop it mid-flight while the provider keeps going.
    let runner = s.runner.clone();
    tokio::spawn(async move {
        if let Err(e) = runner.run_with_guard(guard, TestKind::Custom).await {
            tracing::error!(error = %e, "manual test failed");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({ "status": "started" }))))
}

async fn status(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = config::load(&s.db).await?;
    Ok(Json(json!({
        "running": s.runner.is_running(),
        "paused": cfg.paused,
        "nextRun": s.scheduler.next_run().map(|t| t.timestamp_millis()),
    })))
}

async fn set_paused(s: &AppState, paused: bool) -> ApiResult<StatusCode> {
    config::set(&s.db, "paused", if paused { "true" } else { "false" })
        .await
        .map_err(|e| ApiError::from(anyhow::Error::from(e)))?;
    s.ping_reload();
    Ok(StatusCode::NO_CONTENT)
}

async fn pause(State(s): State<AppState>) -> ApiResult<StatusCode> {
    set_paused(&s, true).await
}

async fn resume(State(s): State<AppState>) -> ApiResult<StatusCode> {
    set_paused(&s, false).await
}

#[derive(Deserialize)]
struct RangeQuery {
    from: Option<String>,
    to: Option<String>,
}

pub(crate) fn parse_date(name: &str, v: Option<&str>) -> Result<NaiveDate, ApiError> {
    let v = v.ok_or_else(|| ApiError::bad_request(format!("missing query parameter: {name}")))?;
    // Four-digit years only: chrono also accepts signed years up to its own
    // limits, where the range end (next midnight) is not representable.
    NaiveDate::parse_from_str(v, "%Y-%m-%d")
        .ok()
        .filter(|d| (1..=9999).contains(&d.year()))
        .ok_or_else(|| ApiError::bad_request(format!("{name} must be a YYYY-MM-DD date")))
}

async fn statistics(
    State(s): State<AppState>,
    ApiQuery(q): ApiQuery<RangeQuery>,
) -> ApiResult<Json<Statistics>> {
    let from = parse_date("from", q.from.as_deref())?;
    let to = parse_date("to", q.to.as_deref())?;
    if to < from {
        return Err(ApiError::bad_request("from must not be after to"));
    }
    Ok(Json(stats::statistics(&s.db, from, to).await?))
}

async fn events(State(s): State<AppState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Subscribe here, not lazily in the stream, so no event after the
    // response is returned can be missed.
    let rx = s.runner.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(event) => match to_sse(&event) {
                    Some(e) => return Some((Ok(e), rx)),
                    None => continue,
                },
                // A slow client just misses some events.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return None,
            }
        }
    });
    let mut stop = s.shutdown.subscribe();
    let stream = stream.take_until(async move {
        let _ = stop.wait_for(|down| *down).await;
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn to_sse(event: &RunEvent) -> Option<Event> {
    Event::default().json_data(event).ok()
}
