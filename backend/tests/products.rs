//! Integration tests for the products system, covering CRUD, QR identity, and authorization.

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
        "POST",
        "/api/setup",
        None,
        json!({ "business_name": "Test", "owner_username": "admin", "owner_password": "password123" }),
    )).await.unwrap();
    body_json(resp).await["token"].as_str().unwrap().to_string()
}

async fn setup_staff(app: &axum::Router, owner_token: &str) -> String {
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/users", Some(owner_token),
        json!({ "username": "staff1", "password": "staffpass123" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app.clone().oneshot(json_request(
        "POST", "/api/auth/login", None,
        json!({ "username": "staff1", "password": "staffpass123" }),
    )).await.unwrap();
    body_json(resp).await["token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn product_crud_and_qr_lookup() {
    let app = test_app();
    let owner_token = setup_owner(&app).await;

    // 1. Unauthorized access is rejected
    let resp = app.clone().oneshot(json_request("GET", "/api/products", None, Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Create product
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&owner_token),
        json!({ "name": "Apple", "qr_code": "QR-APPLE", "price_paise": 10000, "cost_paise": 5000, "gst_rate_bps": 500 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let product_id = body_json(resp).await["product_id"].as_str().unwrap().to_string();

    // 3. Duplicate QR is rejected
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&owner_token),
        json!({ "name": "Fake Apple", "qr_code": "QR-APPLE", "price_paise": 10000, "cost_paise": 5000, "gst_rate_bps": 500 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 4. Invalid product (negative price) rejected
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&owner_token),
        json!({ "name": "Bad", "qr_code": "QR-BAD", "price_paise": -100, "cost_paise": 50, "gst_rate_bps": 500 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 5. Product retrieval
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/products/{}", product_id), Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["name"], "Apple");
    assert_eq!(body["status"], "active");

    // 6. QR lookup
    let resp = app.clone().oneshot(json_request("GET", "/api/products/by-qr/QR-APPLE", Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["name"], "Apple");

    // 7. Unknown QR
    let resp = app.clone().oneshot(json_request("GET", "/api/products/by-qr/UNKNOWN", Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 8. Update product (change QR and price)
    let resp = app.clone().oneshot(json_request(
        "PATCH", &format!("/api/products/{}", product_id), Some(&owner_token),
        json!({ "qr_code": "QR-APPLE-V2", "price_paise": 12000 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Old QR no longer works
    let resp = app.clone().oneshot(json_request("GET", "/api/products/by-qr/QR-APPLE", Some(&owner_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // 10. Deactivate product
    let resp = app.clone().oneshot(json_request(
        "PATCH", &format!("/api/products/{}", product_id), Some(&owner_token),
        json!({ "status": "archived" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["status"], "archived");

    // 11. Search
    app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&owner_token),
        json!({ "name": "Banana", "qr_code": "QR-BANANA", "price_paise": 5000, "cost_paise": 2000, "gst_rate_bps": 500 }),
    )).await.unwrap();
    
    let resp = app.clone().oneshot(json_request("GET", "/api/products?search=Banana", Some(&owner_token), Value::Null)).await.unwrap();
    let search_res = body_json(resp).await;
    let list = search_res.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "Banana");
}

#[tokio::test]
async fn staff_product_access() {
    let app = test_app();
    let owner_token = setup_owner(&app).await;
    let staff_token = setup_staff(&app, &owner_token).await;

    // Staff can create a product
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&staff_token),
        json!({ "name": "Orange", "qr_code": "QR-ORANGE", "price_paise": 8000, "cost_paise": 4000, "gst_rate_bps": 500 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let product_id = body_json(resp).await["product_id"].as_str().unwrap().to_string();

    // Staff can update it
    let resp = app.clone().oneshot(json_request(
        "PATCH", &format!("/api/products/{}", product_id), Some(&staff_token),
        json!({ "price_paise": 9000 }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Staff can list products
    let resp = app.clone().oneshot(json_request("GET", "/api/products", Some(&staff_token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
