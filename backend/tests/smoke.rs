//! End-to-end smoke test driving the real Axum router against an in-memory
//! SQLite DB: setup → login → create staff → create product → QR lookup.
//! Not a substitute for per-module unit tests, but catches wiring mistakes
//! (wrong route, wrong status code, missing auth) cheaply and fast.

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
async fn full_v0_flow() {
    let app = test_app();

    // Health check needs no auth.
    let resp = app.clone().oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // First setup succeeds and returns an owner session token.
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/setup",
            None,
            json!({ "business_name": "Test Shop", "owner_username": "owner1", "owner_password": "ownerpass123" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let owner_token = body["token"].as_str().unwrap().to_string();
    assert_eq!(body["user"]["role"], "owner");

    // Setup again is rejected (already set up).
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/setup",
            None,
            json!({ "business_name": "Other Shop", "owner_username": "owner2", "owner_password": "ownerpass123" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // Owner creates a staff account.
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/users",
            Some(&owner_token),
            json!({ "username": "staff1", "password": "staffpass123" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Staff logs in.
    let resp = app
        .clone()
        .oneshot(json_request("POST", "/api/auth/login", None, json!({ "username": "staff1", "password": "staffpass123" })))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let staff_token = body_json(resp).await["token"].as_str().unwrap().to_string();

    // Staff cannot create other staff accounts (owner-only).
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/users",
            Some(&staff_token),
            json!({ "username": "staff2", "password": "staffpass123" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Staff can create a product with a QR code.
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/products",
            Some(&staff_token),
            json!({ "name": "Rice 1kg", "gst_rate_bps": 500, "qr_code": "QR-RICE-1KG", "price_paise": 8000, "cost_paise": 6000 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let product = body_json(resp).await;
    assert_eq!(product["qr_code"], "QR-RICE-1KG");

    // Duplicate QR code is rejected.
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/products",
            Some(&staff_token),
            json!({ "name": "Rice 5kg", "gst_rate_bps": 500, "qr_code": "QR-RICE-1KG", "price_paise": 35000, "cost_paise": 30000 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // QR lookup resolves the product (the core of QR-first billing).
    let resp = app
        .clone()
        .oneshot(json_request("GET", "/api/products/by-qr/QR-RICE-1KG", Some(&staff_token), Value::Null))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // No token at all is rejected.
    let resp = app.clone().oneshot(Request::builder().uri("/api/products").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
