use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use chrono::Utc;
use futures::FutureExt;
use futures::future::BoxFuture;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::providers::{Measurement, ProviderError, RunOptions, SpeedProvider};
use netrack::runner::{ProviderFactory, Runner};
use netrack::scheduler::Scheduler;
use serde_json::{Value, json};
use tokio::sync::watch;
use tower::ServiceExt;

struct DummyProvider;

impl SpeedProvider for DummyProvider {
    fn name(&self) -> &'static str {
        "dummy"
    }

    fn run<'a>(&'a self, _: &'a RunOptions) -> BoxFuture<'a, Result<Measurement, ProviderError>> {
        async move {
            Ok(Measurement {
                ping: 10.0,
                jitter: 1.0,
                download: 100.0,
                upload: 50.0,
                server_id: Some("1".into()),
                server_name: Some("Test".into()),
                server_host: Some("localhost".into()),
                result_id: None,
            })
        }
        .boxed()
    }
}

struct TestApp {
    app: Router,
    db: Db,
}

impl TestApp {
    async fn new() -> Self {
        let db = Db::open_in_memory(chrono_tz::Tz::UTC).await.unwrap();
        let factory: ProviderFactory = Arc::new(|_| Box::new(DummyProvider));
        let runner = Runner::new(db.clone(), factory);
        let (tx, rx) = watch::channel(());
        let scheduler = Scheduler::spawn(db.clone(), runner.clone(), rx);
        let state = AppState::new(db.clone(), runner, tx, Arc::new(scheduler));
        let app = router(state);
        Self { app, db }
    }

    async fn request(
        &self,
        method: Method,
        uri: &str,
        body: Option<Value>,
        cookie: Option<&str>,
        auth_header: Option<&str>,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            req = req.header("cookie", c);
        }
        if let Some(a) = auth_header {
            req = req.header("authorization", a);
        }
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };

        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }
}

#[tokio::test]
async fn auth_status_default_is_none() {
    let app = TestApp::new().await;
    let (status, _, json) = app
        .request(Method::GET, "/api/auth/status", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["mode"], "none");
    assert_eq!(json["authenticated"], false);
}

#[tokio::test]
async fn direct_password_mode_patch_is_rejected() {
    let app = TestApp::new().await;
    let (status, _, json) = app
        .request(
            Method::PATCH,
            "/api/config/password_mode",
            Some(json!({ "value": "protected" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        json["error"]
            .as_str()
            .unwrap_or("")
            .contains("cannot set password_mode directly")
    );
}

#[tokio::test]
async fn set_password_and_public_mode_enforcement() {
    let app = TestApp::new().await;

    // 1. Set password with mode "public"
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "password": "supersecretpassword", "mode": "public" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // 2. Status shows public mode, unauthenticated
    let (status, _, json) = app
        .request(Method::GET, "/api/auth/status", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["mode"], "public");
    assert_eq!(json["authenticated"], false);

    // 3. GET requests on /api/* are allowed in public mode without auth
    let (status, _, _) = app
        .request(Method::GET, "/api/speedtests", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, _) = app
        .request(Method::GET, "/api/config", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);

    // 4. Mutating requests on /api/* return 401 without auth
    let (status, _, json) = app
        .request(Method::POST, "/api/speedtests/run", None, None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    let (status, _, json) = app
        .request(
            Method::PATCH,
            "/api/config/retention_days",
            Some(json!({ "value": "180" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    let (status, _, json) = app
        .request(Method::DELETE, "/api/speedtests/999", None, None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    // 5. Login with wrong password returns 401
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "wrongpassword" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // 6. Login with correct password succeeds and sets session cookie
    let (status, headers, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "supersecretpassword" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = json["token"].as_str().expect("token in response");
    assert!(!token.is_empty());

    let cookie_header = headers
        .get("set-cookie")
        .expect("set-cookie header present")
        .to_str()
        .unwrap();
    assert!(cookie_header.contains("netrack_session="));

    // 7. Mutating requests work with cookie
    let cookie_val = format!("netrack_session={token}");
    let (status, _, _) = app
        .request(
            Method::PATCH,
            "/api/config/retention_days",
            Some(json!({ "value": "180" })),
            Some(&cookie_val),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 8. Mutating requests work with Authorization: Bearer <token>
    let bearer = format!("Bearer {token}");
    let (status, _, _) = app
        .request(
            Method::PATCH,
            "/api/config/retention_days",
            Some(json!({ "value": "180" })),
            None,
            Some(&bearer),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // 9. Status reports authenticated = true
    let (status, _, json) = app
        .request(
            Method::GET,
            "/api/auth/status",
            None,
            Some(&cookie_val),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["authenticated"], true);

    // 10. Logout invalidates session
    let (status, headers, _) = app
        .request(
            Method::POST,
            "/api/auth/logout",
            None,
            Some(&cookie_val),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let cookie_logout = headers.get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie_logout.contains("netrack_session="));
    assert!(cookie_logout.contains("Max-Age=0"));

    // 11. Previously valid token is now rejected
    let (status, _, json) = app
        .request(
            Method::POST,
            "/api/speedtests/run",
            None,
            Some(&cookie_val),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn protected_mode_blocks_all_api_except_exemptions() {
    let app = TestApp::new().await;

    // Set protected mode
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "password": "mypassword123", "mode": "protected" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // 1. Exemptions work without auth
    let (status, _, _) = app.request(Method::GET, "/healthz", None, None, None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, json) = app
        .request(Method::GET, "/api/auth/status", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["mode"], "protected");
    assert_eq!(json["authenticated"], false);

    // Static assets / SPA routes work without auth
    let (status, _, _) = app.request(Method::GET, "/", None, None, None).await;
    assert_eq!(status, StatusCode::OK);

    // 2. All /api/* data endpoints are rejected without auth
    let (status, _, json) = app
        .request(Method::GET, "/api/speedtests", None, None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    let (status, _, json) = app
        .request(Method::GET, "/api/config", None, None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    let (status, _, json) = app
        .request(Method::POST, "/api/speedtests/run", None, None, None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));

    // 3. Login returns session token
    let (status, _, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "mypassword123" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = json["token"].as_str().unwrap();

    // 4. Authenticated request now succeeds on data endpoints
    let cookie = format!("netrack_session={token}");
    let (status, _, _) = app
        .request(Method::GET, "/api/config", None, Some(&cookie), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn password_change_and_removal() {
    let app = TestApp::new().await;

    // Set initial password
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "password": "firstpassword", "mode": "protected" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Login with initial password
    let (status, _, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "firstpassword" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let token = json["token"].as_str().unwrap().to_owned();
    let auth = format!("Bearer {token}");

    // Change password using authenticated session
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "password": "secondpassword" })),
            None,
            Some(&auth),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Old password no longer works
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "firstpassword" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // New password works
    let (status, _, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "secondpassword" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let new_token = json["token"].as_str().unwrap().to_owned();
    let new_auth = format!("Bearer {new_token}");

    // Remove password: mode "none"
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "mode": "none" })),
            None,
            Some(&new_auth),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Now in mode "none", requests succeed without auth
    let (status, _, json) = app
        .request(Method::GET, "/api/auth/status", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["mode"], "none");

    let (status, _, _) = app
        .request(Method::GET, "/api/config", None, None, None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn expired_session_token_is_rejected() {
    let app = TestApp::new().await;

    // Set protected mode
    app.request(
        Method::POST,
        "/api/auth/password",
        Some(json!({ "password": "password123", "mode": "protected" })),
        None,
        None,
    )
    .await;

    // Directly insert an expired session into DB
    let expired_token = "expired_test_token_1234567890abcdef";
    let now = Utc::now().timestamp();
    let expired_at = now - 3600; // 1 hour ago
    app.db
        .call(move |conn| {
            conn.execute(
                "INSERT INTO sessions (token, created_at, expires_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![expired_token, now - 7200, expired_at],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    // Using expired token returns 401
    let auth = format!("Bearer {expired_token}");
    let (status, _, json) = app
        .request(Method::GET, "/api/config", None, None, Some(&auth))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(json, json!({ "error": "unauthorized" }));
}

#[tokio::test]
async fn public_mode_hides_secrets_from_anonymous() {
    let app = TestApp::new().await;
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/password",
            Some(json!({ "password": "pw", "mode": "public" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    for uri in [
        "/api/integrations",
        "/api/nodes",
        "/api/data/export",
        "/api/data/stats",
    ] {
        let (status, _, json) = app.request(Method::GET, uri, None, None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{uri}");
        assert_eq!(json, json!({ "error": "unauthorized" }), "{uri}");
    }
    for uri in ["/api/speedtests", "/api/config"] {
        let (status, _, _) = app.request(Method::GET, uri, None, None, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
    }

    let (status, headers, _) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "pw" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let (status, _, _) = app
        .request(Method::GET, "/api/integrations", None, Some(&cookie), None)
        .await;
    assert_eq!(status, StatusCode::OK);
}

async fn protect(app: &TestApp) {
    app.request(
        Method::POST,
        "/api/auth/password",
        Some(json!({ "password": "password123", "mode": "protected" })),
        None,
        None,
    )
    .await;
}

#[tokio::test]
async fn wrong_password_login_is_delayed() {
    let app = TestApp::new().await;
    protect(&app).await;
    let start = std::time::Instant::now();
    let (status, _, _) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "wrong" })),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(start.elapsed() >= std::time::Duration::from_millis(450));
}

#[tokio::test]
async fn session_token_does_not_hit_argon() {
    let app = TestApp::new().await;
    protect(&app).await;
    let (_, _, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "password123" })),
            None,
            None,
        )
        .await;
    let cookie = format!("netrack_session={}", json["token"].as_str().unwrap());
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let (status, _, _) = app
            .request(Method::GET, "/api/speedtests", None, Some(&cookie), None)
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[tokio::test]
async fn expired_cookie_is_rejected_fast() {
    let app = TestApp::new().await;
    protect(&app).await;
    let start = std::time::Instant::now();
    let (status, _, _) = app
        .request(
            Method::GET,
            "/api/speedtests",
            None,
            Some("netrack_session=deadbeef"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(start.elapsed() < std::time::Duration::from_millis(200));
}

#[tokio::test]
async fn bearer_session_token_is_fast() {
    let app = TestApp::new().await;
    protect(&app).await;
    let (_, _, json) = app
        .request(
            Method::POST,
            "/api/auth/login",
            Some(json!({ "password": "password123" })),
            None,
            None,
        )
        .await;
    let auth = format!("Bearer {}", json["token"].as_str().unwrap());
    let start = std::time::Instant::now();
    for _ in 0..20 {
        let (status, _, _) = app
            .request(Method::GET, "/api/speedtests", None, None, Some(&auth))
            .await;
        assert_eq!(status, StatusCode::OK);
    }
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
}

#[tokio::test]
async fn cookie_and_bearer_password_from_old_primary_is_accepted() {
    let app = TestApp::new().await;
    protect(&app).await;
    let (status, _, _) = app
        .request(
            Method::GET,
            "/api/speedtests",
            None,
            Some("netrack_session=password123"),
            Some("Bearer password123"),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}
