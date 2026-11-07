use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{any, get};
use serde::Deserialize;

use super::{NodeResponse, NodeStatusResponse};
use crate::api::{ApiError, ApiJson, ApiPath, ApiResult, AppState};
use crate::db::tests_repo::SpeedtestRow;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/nodes", get(list_nodes).post(create_node))
        .route(
            "/api/nodes/{id}",
            get(get_node)
                .put(update_node)
                .patch(update_node)
                .delete(delete_node),
        )
        .route("/api/nodes/{id}/status", get(node_status))
        .route("/api/nodes/{id}/proxy", any(proxy_root))
        .route("/api/nodes/{id}/proxy/{*path}", any(proxy_path))
}

fn validate_url(url: &str) -> Result<String, ApiError> {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err(ApiError::bad_request(
            "url must start with http:// or https://",
        ));
    }
    if let Err(e) = reqwest::Url::parse(trimmed) {
        return Err(ApiError::bad_request(format!("invalid url: {e}")));
    }
    Ok(trimmed.trim_end_matches('/').to_string())
}

#[derive(Debug, Deserialize)]
pub struct CreateNodePayload {
    pub name: String,
    pub url: String,
    pub password: Option<String>,
}

async fn list_nodes(State(s): State<AppState>) -> ApiResult<Json<Vec<NodeResponse>>> {
    let nodes = super::list(&s.db).await?;
    let resp: Vec<NodeResponse> = nodes.into_iter().map(NodeResponse::from).collect();
    Ok(Json(resp))
}

async fn create_node(
    State(s): State<AppState>,
    ApiJson(body): ApiJson<CreateNodePayload>,
) -> ApiResult<(StatusCode, Json<NodeResponse>)> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name cannot be empty"));
    }
    let url = validate_url(&body.url)?;
    let password = body
        .password
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let node = super::insert(&s.db, name.to_string(), url, password).await?;
    Ok((StatusCode::CREATED, Json(NodeResponse::from(node))))
}

async fn get_node(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<Json<NodeResponse>> {
    let node = match super::get(&s.db, id).await? {
        Some(n) => n,
        None => return Err(ApiError::new(StatusCode::NOT_FOUND, "node not found")),
    };
    Ok(Json(NodeResponse::from(node)))
}

#[derive(Debug, Deserialize)]
pub struct UpdateNodePayload {
    pub name: Option<String>,
    pub url: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_opt")]
    pub password: Option<Option<String>>,
}

fn deserialize_opt_opt<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::deserialize(deserializer)?))
}

async fn update_node(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
    ApiJson(body): ApiJson<UpdateNodePayload>,
) -> ApiResult<Json<NodeResponse>> {
    let new_name = match body.name {
        Some(n) => {
            let trimmed = n.trim();
            if trimmed.is_empty() {
                return Err(ApiError::bad_request("name cannot be empty"));
            }
            Some(trimmed.to_string())
        }
        None => None,
    };

    let new_url = match body.url {
        Some(u) => Some(validate_url(&u)?),
        None => None,
    };

    let new_password = match body.password {
        Some(Some(pwd)) => {
            let trimmed = pwd.trim().to_string();
            if trimmed.is_empty() {
                Some(None)
            } else {
                Some(Some(trimmed))
            }
        }
        Some(None) => Some(None),
        None => None,
    };

    let updated = super::update(&s.db, id, new_name, new_url, new_password).await?;
    match updated {
        Some(node) => Ok(Json(NodeResponse::from(node))),
        None => Err(ApiError::new(StatusCode::NOT_FOUND, "node not found")),
    }
}

async fn delete_node(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<StatusCode> {
    let deleted = super::delete(&s.db, id).await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "node not found"))
    }
}

async fn node_status(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
) -> ApiResult<Json<NodeStatusResponse>> {
    let node = match super::get(&s.db, id).await? {
        Some(n) => n,
        None => return Err(ApiError::new(StatusCode::NOT_FOUND, "node not found")),
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let base_url = node.url.trim_end_matches('/');
    let status_url = format!("{base_url}/api/system/status");
    let speedtests_url = format!("{base_url}/api/speedtests?limit=1");

    let mut status_req = client.get(&status_url);
    let mut speedtests_req = client.get(&speedtests_url);

    if let Some(pwd) = &node.password
        && !pwd.trim().is_empty()
    {
        status_req = status_req.header(header::AUTHORIZATION, format!("Bearer {pwd}"));
        speedtests_req = speedtests_req.header(header::AUTHORIZATION, format!("Bearer {pwd}"));
    }

    let status_resp = status_req.send().await;
    let (online, version, error) = match status_resp {
        Ok(resp) if resp.status().is_success() => {
            let bytes = resp.bytes().await.unwrap_or_default();
            let body: serde_json::Value =
                serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            let version = body
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            (true, version, None)
        }
        Ok(resp) => {
            let status = resp.status();
            (
                false,
                None,
                Some(format!("remote returned status {status}")),
            )
        }
        Err(e) => (false, None, Some(e.to_string())),
    };

    let mut latest_test = None;
    if online
        && let Ok(resp) = speedtests_req.send().await
        && resp.status().is_success()
        && let Ok(bytes) = resp.bytes().await
        && let Ok(rows) = serde_json::from_slice::<Vec<SpeedtestRow>>(&bytes)
    {
        latest_test = rows.into_iter().next();
    }

    Ok(Json(NodeStatusResponse {
        online,
        version,
        latest_test,
        error,
    }))
}

#[derive(Deserialize)]
struct ProxyPathParams {
    id: i64,
    path: String,
}

async fn proxy_root(
    State(s): State<AppState>,
    ApiPath(id): ApiPath<i64>,
    req: Request,
) -> Result<Response, ApiError> {
    proxy_req(&s, id, "", req).await
}

async fn proxy_path(
    State(s): State<AppState>,
    ApiPath(params): ApiPath<ProxyPathParams>,
    req: Request,
) -> Result<Response, ApiError> {
    proxy_req(&s, params.id, &params.path, req).await
}

async fn proxy_req(
    state: &AppState,
    id: i64,
    path: &str,
    req: Request,
) -> Result<Response, ApiError> {
    let node = match super::get(&state.db, id).await? {
        Some(n) => n,
        None => return Err(ApiError::new(StatusCode::NOT_FOUND, "node not found")),
    };

    let base_url = node.url.trim_end_matches('/');
    let subpath = path.trim_start_matches('/');
    let subpath = subpath.strip_prefix("api/").unwrap_or(subpath);
    let target_path = if subpath.is_empty() || subpath == "api" {
        format!("{base_url}/api")
    } else {
        format!("{base_url}/api/{subpath}")
    };

    let target_url = if let Some(query) = req.uri().query() {
        format!("{target_path}?{query}")
    } else {
        target_path
    };

    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .map_err(|e| ApiError::bad_request(e.to_string()))?;

    let mut req_builder = client.request(method, &target_url);

    let orig_cookie = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok());

    for (name, val) in req.headers() {
        let name_str = name.as_str().to_lowercase();
        if matches!(
            name_str.as_str(),
            "host"
                | "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
                | "content-length"
                | "cookie"
                | "authorization"
        ) {
            continue;
        }
        req_builder = req_builder.header(name.clone(), val.clone());
    }

    if let Some(pwd) = &node.password
        && !pwd.trim().is_empty()
    {
        req_builder = req_builder.header(header::AUTHORIZATION, format!("Bearer {pwd}"));
    }
    if let Some(cookie_str) = orig_cookie {
        let parts: Vec<&str> = cookie_str
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.starts_with("netrack_session="))
            .collect();
        if !parts.is_empty() {
            req_builder = req_builder.header(header::COOKIE, parts.join("; "));
        }
    }

    let body = req.into_body();
    let reqwest_body = reqwest::Body::wrap_stream(body.into_data_stream());
    req_builder = req_builder.body(reqwest_body);

    let resp = req_builder.send().await.map_err(|e| {
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("failed to proxy request to node: {e}"),
        )
    })?;

    let status =
        StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

    let mut response_builder = Response::builder().status(status);
    for (k, v) in resp.headers() {
        let name_str = k.as_str().to_lowercase();
        if matches!(
            name_str.as_str(),
            "connection"
                | "keep-alive"
                | "proxy-authenticate"
                | "proxy-authorization"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        ) {
            continue;
        }
        response_builder = response_builder.header(k.clone(), v.clone());
    }

    let body_stream = resp.bytes_stream();
    let axum_body = Body::from_stream(body_stream);
    let response = response_builder
        .body(axum_body)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(response)
}
