//! Integration tests for the Purchase & OCR Workflow (Phase 06)

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

#[tokio::test]
async fn test_purchase_workflow() {
    let app = test_app();
    let token = setup_owner(&app).await;

    // 1. Create product
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/products", Some(&token),
        json!({ "name": "Rice", "qr_code": "QR-RICE", "price_paise": 5000, "cost_paise": 4000, "gst_rate_bps": 500 }),
    )).await.unwrap();
    let product_id = body_json(resp).await["product_id"].as_str().unwrap().to_string();

    // 2. Create supplier
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/suppliers", Some(&token),
        json!({ "name": "Farms Co" }),
    )).await.unwrap();
    let supplier_id = body_json(resp).await["supplier_id"].as_str().unwrap().to_string();

    // 3. Create Draft Purchase
    let resp = app.clone().oneshot(json_request(
        "POST", "/api/purchases", Some(&token),
        json!({ "source": "manual" }),
    )).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let purchase_id = body_json(resp).await["purchase_id"].as_str().unwrap().to_string();

    // 4. Update Draft with unresolved product (simulate OCR output before matching)
    let update_req = json!({
        "supplier_id": supplier_id,
        "invoice_no": "INV-123",
        "invoice_date": "2026-10-06",
        "items": [
            {
                "product_id": null, // Unresolved
                "product_name_snapshot": "Rice 1Kg Bag OCR",
                "quantity_milli": 10_000,
                "price_paise": 4000,
                "gst_rate_bps": 500
            }
        ]
    });
    let resp = app.clone().oneshot(json_request("PATCH", &format!("/api/purchases/{}", purchase_id), Some(&token), update_req)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Try Approve -> Should Fail (Unmatched Product)
    let resp = app.clone().oneshot(json_request("POST", &format!("/api/purchases/{}/approve", purchase_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 6. Inventory should remain 0
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 0);

    // 7. Update Draft to map the product
    let update_req = json!({
        "supplier_id": supplier_id,
        "invoice_no": "INV-123",
        "items": [
            {
                "product_id": product_id, // Resolved!
                "product_name_snapshot": "Rice",
                "quantity_milli": 10_000,
                "price_paise": 4000,
                "gst_rate_bps": 500
            }
        ]
    });
    let resp = app.clone().oneshot(json_request("PATCH", &format!("/api/purchases/{}", purchase_id), Some(&token), update_req)).await.unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["total_paise"], 42000); // 40000 + 2000 tax (5%)

    // 8. Approve Purchase
    let resp = app.clone().oneshot(json_request("POST", &format!("/api/purchases/{}/approve", purchase_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Check Inventory -> Should be 10_000
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 10_000);

    // 10. Duplicate Approval -> Fails cleanly with Conflict (Idempotency)
    let resp = app.clone().oneshot(json_request("POST", &format!("/api/purchases/{}/approve", purchase_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 11. Edit Approved -> Fails (Immutable after approval)
    let resp = app.clone().oneshot(json_request("PATCH", &format!("/api/purchases/{}", purchase_id), Some(&token), json!({ "items": [] }))).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 12. Create another draft and cancel it
    let resp = app.clone().oneshot(json_request("POST", "/api/purchases", Some(&token), json!({ "source": "ocr" }))).await.unwrap();
    let draft_id2 = body_json(resp).await["purchase_id"].as_str().unwrap().to_string();

    let resp = app.clone().oneshot(json_request("POST", &format!("/api/purchases/{}/cancel", draft_id2), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Cancelled draft shouldn't affect inventory
    let resp = app.clone().oneshot(json_request("GET", &format!("/api/inventory/{}", product_id), Some(&token), Value::Null)).await.unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 10_000); // unaffected
}
