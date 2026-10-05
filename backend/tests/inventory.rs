//! Integration tests for the inventory transaction engine (Phase 05).

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

async fn setup_owner(app: &axum::Router) -> String {
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/setup", None,
        json!({ "business_name": "Test Shop", "owner_username": "admin", "owner_password": "password123" }),
    )).await.unwrap();
    body_json(resp).await["token"].as_str().unwrap().to_string()
}

async fn create_product(app: &axum::Router, token: &str, name: &str, qr: &str) -> String {
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(token),
        json!({ "name": name, "qr_code": qr, "price_paise": 1000, "cost_paise": 500, "gst_rate_bps": 500 }),
    )).await.unwrap();
    body_json(resp).await["product_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn inventory_transactions_and_constraints() {
    let app = test_app();
    let token = setup_owner(&app).await;
    let product_id = create_product(&app, &token, "Widget", "QR-WIDGET").await;

    // 1. Opening stock
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/opening", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 10_000 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 2. Duplicate opening stock rejected
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/opening", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 5_000 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 3. Positive stock calculation
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["current_stock_milli"], 10_000);

    // 4. Positive adjustment
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 5_000, "reason": "Found extra box" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Negative adjustment (Sale-style movement)
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": -2_000, "reason": "Damaged goods" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. Check stock is now 13_000
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 13_000);

    // 7. Negative stock rejection
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": -15_000, "reason": "Big sale" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 8. Adjustment with no reason rejected
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": -1_000, "reason": "" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 9. Zero quantity rejected
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 0, "reason": "Nothing" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 10. Check history
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}/history", product_id), Some(&token), Value::Null)).await.unwrap();
    let history = body_json(resp).await;
    let list = history.as_array().unwrap();
    assert_eq!(list.len(), 3); // Opening, Adjust +5, Adjust -2
    assert_eq!(list[0]["quantity_milli"], -2_000); // DESC ordering
    assert_eq!(list[1]["quantity_milli"], 5_000);
    assert_eq!(list[2]["quantity_milli"], 10_000);

    // 11. Archived product rejection
    // Archive product
    app.clone().oneshot(json_request(
        "PATCH", &format!("/api/products/{}", product_id), Some(&token),
        json!({ "status": "archived" }),
    )).await.unwrap();

    // Try adjustment
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 1_000, "reason": "Try on archived" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 12. Unauthorized
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/inventory/adjust", None,
        json!({ "product_id": product_id, "quantity_milli": 1_000, "reason": "No token" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn inventory_concurrency_protection() {
    let conn = db::open_in_memory();
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);
    
    let token = setup_owner(&app).await;
    let product_id = create_product(&app, &token, "Stock Limit", "QR-LIMIT").await;

    // Set opening stock to 5 units
    app.clone().oneshot(json_request(
        "POST", "/api/inventory/opening", Some(&token),
        json!({ "product_id": product_id, "quantity_milli": 5_000 }),
    )).await.unwrap();

    // Spawn 10 concurrent requests subtracting 1 unit each.
    // SQLite locking via Mutex enforces serial execution, so the first 5 should succeed and the next 5 should fail with BAD_REQUEST (insufficient stock).
    let mut futures = vec![];
    for _ in 0..10 {
        let req = json_request(
            "POST", "/api/inventory/adjust", Some(&token),
            json!({ "product_id": product_id, "quantity_milli": -1_000, "reason": "Concurrent drain" }),
        );
        futures.push(app.clone().oneshot(req));
    }

    let results = futures::future::join_all(futures).await;
    let mut successes = 0;
    let mut failures = 0;

    for res in results {
        let resp = res.unwrap();
        if resp.status() == StatusCode::OK {
            successes += 1;
        } else if resp.status() == StatusCode::BAD_REQUEST {
            failures += 1;
        }
    }

    assert_eq!(successes, 5);
    assert_eq!(failures, 5);

    // Final balance should be exactly 0.
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 0);
}
