//! Detailed authentication tests covering edge cases and security boundaries.

use std::sync::{Arc, Mutex};
use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_app() -> axum::Router {
    let conn = db::open_in_memory();
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    build_app(state)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn json_request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri).header("content-type", "application/json");
    if let Some(t) = token {
        builder = builder.header("authorization", format!("Bearer {t}"));
    }
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn auth_edge_cases_and_security() {
    let app = test_app();

    // 1. Owner setup succeeds
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/setup",
        None,
        json!({ "business_name": "Test", "owner_username": "admin", "owner_password": "password123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let owner_token = body_json(resp).await["token"].as_str().unwrap().to_string();

    // 2. Owner setup cannot be repeated
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/setup",
        None,
        json!({ "business_name": "Test 2", "owner_username": "admin2", "owner_password": "password123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 3. Incorrect password fails
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/auth/login",
        None,
        json!({ "username": "admin", "password": "wrongpassword" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 4. Nonexistent user fails
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/auth/login",
        None,
        json!({ "username": "nobody", "password": "password123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Create a staff user
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/users",
        Some(&owner_token),
        json!({ "username": "staff1", "password": "staffpass123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let staff_id = body_json(resp).await["user_id"].as_str().unwrap().to_string();

    // Disable the staff user
    let resp = app.clone().oneshot(json_request(
        "PATCH",
        &format!("/api/users/{}", staff_id),
        Some(&owner_token),
        json!({ "status": "disabled" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Disabled user cannot log in
    let resp = app.clone().oneshot(json_request(
        "POST",
        "/api/auth/login",
        None,
        json!({ "username": "staff1", "password": "staffpass123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 6. Invalid session is rejected
    let resp = app.clone().oneshot(json_request("GET", "/api/auth/me", Some("invalid_token_123"), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 7. Logout invalidates session
    let resp = app.clone().oneshot(json_request("POST", "/api/auth/logout", Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app.clone().oneshot(json_request("GET", "/api/auth/me", Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn duplicate_setup_race_condition() {
    // We simulate a race condition where multiple setups try to occur at the same time.
    // However, SQLite with a mutex forces serial execution of the setup requests at the DB layer,
    // so we just test that sending multiple setups concurrently still results in only ONE success.
    let conn = db::open_in_memory();
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);

    let mut futures = vec![];
    for i in 0..10 {
        let req = json_request(
            "POST",
            "/api/setup",
            None,
            json!({ "business_name": format!("Test {}", i), "owner_username": format!("admin{}", i), "owner_password": "password123" }),
        );
        futures.push(app.clone().oneshot(req));
    }

    let results = futures::future::join_all(futures).await;
    let mut successes = 0;
    let mut conflicts = 0;

    for res in results {
        let resp = res.unwrap();
        if resp.status() == StatusCode::OK {
            successes += 1;
        } else if resp.status() == StatusCode::CONFLICT {
            conflicts += 1;
        }
    }

    assert_eq!(successes, 1);
    assert_eq!(conflicts, 9);
}
