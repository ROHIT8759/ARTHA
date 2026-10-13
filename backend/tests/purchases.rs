//! Integration tests for the Purchase & OCR Workflow (Phase 06)

use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_app() -> axum::Router {
    let conn = db::open_in_memory();
    let state = AppState::for_tests(conn);
    build_app(state)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

fn json_request(method: &str, uri: &str, token: Option<&str>, body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
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
    let product_id = body_json(resp).await["product_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Create supplier
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/suppliers",
            Some(&token),
            json!({ "name": "Farms Co" }),
        ))
        .await
        .unwrap();
    let supplier_id = body_json(resp).await["supplier_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Create Draft Purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "manual" }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

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
    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{}", purchase_id),
            Some(&token),
            update_req,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Try Approve -> Should Fail (Unmatched Product)
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{}/approve", purchase_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 6. Inventory should remain 0
    let resp = app
        .clone()
        .oneshot(json_request(
            "GET",
            &format!("/api/inventory/{}", product_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
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
    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{}", purchase_id),
            Some(&token),
            update_req,
        ))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["total_paise"], 42000); // 40000 + 2000 tax (5%)

    // 8. Approve Purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{}/approve", purchase_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 9. Check Inventory -> Should be 10_000
    let resp = app
        .clone()
        .oneshot(json_request(
            "GET",
            &format!("/api/inventory/{}", product_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 10_000);

    // 10. Duplicate Approval -> Fails cleanly with Conflict (Idempotency)
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{}/approve", purchase_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // 11. Edit Approved -> Fails (Immutable after approval)
    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{}", purchase_id),
            Some(&token),
            json!({ "items": [] }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // 12. Create another draft and cancel it
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let draft_id2 = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{}/cancel", draft_id2),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Cancelled draft shouldn't affect inventory
    let resp = app
        .clone()
        .oneshot(json_request(
            "GET",
            &format!("/api/inventory/{}", product_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 10_000); // unaffected
}

fn multipart_image_request(
    uri: &str,
    token: &str,
    file_name: &str,
    file_bytes: Vec<u8>,
    content_type: &str,
) -> Request<Body> {
    let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
    let mut body_bytes = Vec::new();
    body_bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body_bytes.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body_bytes.extend_from_slice(&file_bytes);
    body_bytes.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {token}"))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body_bytes))
        .unwrap()
}

#[tokio::test]
async fn test_bill_image_upload_magic_bytes_and_retrieval() {
    let app = test_app();
    let token = setup_owner(&app).await;

    // 1. Create a draft purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Upload invalid spoofed file (e.g. text/html pretending to be image) -> 400 Bad Request
    let evil_payload = b"<html><script>alert(1)</script></html>".to_vec();
    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill.png",
            evil_payload,
            "image/png",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 3. Upload valid PNG image with genuine raster magic bytes
    let valid_png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "invoice.png",
            valid_png.clone(),
            "image/png",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let upload_json = body_json(resp).await;
    assert_eq!(upload_json["mime"], "image/png");
    let image_id = upload_json["image_id"].as_str().unwrap().to_string();

    // 4. Retrieve image via GET -> returns 200, Content-Type image/png and identical bytes
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/purchases/{purchase_id}/image"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("content-type").unwrap(), "image/png");
    let retrieved_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(retrieved_bytes.as_ref(), valid_png.as_slice());

    // 5. Get purchase view includes image_id
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/purchases/{purchase_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["image_id"], image_id);
}

#[tokio::test]
async fn test_ocr_pipeline_and_human_review_workflow() {
    let app = test_app();
    let token = setup_owner(&app).await;

    // Create a product to match against later
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/products",
            Some(&token),
            json!({ "name": "Basmati Rice 5kg", "qr_code": "QR-RICE-5", "price_paise": 40000, "cost_paise": 35000, "gst_rate_bps": 500 }),
        ))
        .await
        .unwrap();
    let rice_id = body_json(resp).await["product_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 1. Create draft purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Upload valid JPEG bill image
    let valid_jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill.jpg",
            valid_jpeg,
            "image/jpeg",
        ))
        .await
        .unwrap();
    let image_id = body_json(resp).await["image_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 3. Trigger OCR process
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/ocr"),
            Some(&token),
            json!({ "image_id": image_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ocr_res = body_json(resp).await;
    assert_eq!(ocr_res["status"], "completed");
    assert_eq!(ocr_res["parsed"]["invoice_no"], "INV-2026-089");
    assert_eq!(ocr_res["parsed"]["invoice_date"], "2026-10-12");

    let parsed_items = ocr_res["parsed"]["items"].as_array().unwrap();
    assert!(!parsed_items.is_empty());

    // Invariant check: OCR does NOT mutate purchase total_paise or inventory!
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/purchases/{purchase_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let purchase_view = body_json(resp).await;
    assert_eq!(purchase_view["total_paise"], 0);
    assert_eq!(purchase_view["status"], "draft");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/inventory/{rice_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 0);

    // 4. Staff applies extracted OCR data to draft with unresolved product
    let first_item = &parsed_items[0];
    let draft_items = vec![json!({
        "product_id": null, // Unmatched initially
        "product_name_snapshot": first_item["product_name_snapshot"].as_str().unwrap(),
        "quantity_milli": first_item["quantity_milli"].as_i64().unwrap(),
        "price_paise": first_item["price_paise"].as_i64().unwrap(),
        "gst_rate_bps": first_item["gst_rate_bps"].as_i64().unwrap(),
    })];

    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{purchase_id}"),
            Some(&token),
            json!({
                "invoice_no": ocr_res["parsed"]["invoice_no"].as_str().unwrap(),
                "invoice_date": ocr_res["parsed"]["invoice_date"].as_str().unwrap(),
                "items": draft_items,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Invariant check: approve fails because item is unmatched
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/approve"),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 5. Staff matches the item to our catalog product and approves
    let resolved_items = vec![json!({
        "product_id": rice_id, // Matched!
        "product_name_snapshot": "Basmati Rice 5kg",
        "quantity_milli": 5000,
        "price_paise": 35000,
        "gst_rate_bps": 500,
    })];

    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{purchase_id}"),
            Some(&token),
            json!({
                "invoice_no": "INV-2026-089",
                "items": resolved_items,
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated_view = body_json(resp).await;
    // 5 units @ 35000 paise = 175000 subtotal + 5% tax (8750) = 183750
    assert_eq!(updated_view["total_paise"], 183750);

    // Approve purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/approve"),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Invariant check: stock incremented accurately only on approval!
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/api/inventory/{rice_id}"))
                .header("authorization", format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body_json(resp).await["current_stock_milli"], 5000);
}

#[tokio::test]
async fn test_route_specific_upload_body_limits() {
    let app = test_app();
    let token = setup_owner(&app).await;

    // 1. Create a draft purchase
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    // 2. Upload valid PNG image of ~2.5 MiB (exceeds Axum's default 2 MiB, but under 10 MiB limit)
    let png_header: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut large_valid_png = Vec::with_capacity(2_500_000);
    large_valid_png.extend_from_slice(&png_header);
    large_valid_png.resize(2_500_000, 0x00);

    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill_2_5mb.png",
            large_valid_png,
            "image/png",
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Valid 2.5 MiB image upload must succeed under the route's 10 MiB limit"
    );

    // 3. Upload oversized image of ~10.5 MiB (exceeds 10 MiB limit) -> 413 Payload Too Large
    let mut oversized_png = Vec::with_capacity(10_500_000);
    oversized_png.extend_from_slice(&png_header);
    oversized_png.resize(10_500_000, 0x00);

    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill_10_5mb.png",
            oversized_png,
            "image/png",
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "Oversized 10.5 MiB image must be rejected with 413 Payload Too Large"
    );

    // 4. Send 2.5 MiB body to an unrelated route (POST /api/purchases) -> 413 Payload Too Large
    // Confirms that the 10 MiB limit is strictly route-specific and unrelated routes retain the 2 MiB default
    let big_padding = "x".repeat(2_500_000);
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "invoice_no": big_padding }),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "Unrelated route must retain Axum's default 2 MiB limit and reject 2.5 MiB payloads"
    );
}

#[tokio::test]
async fn test_ocr_production_mode_without_cmd_preserves_invariants() {
    let conn = db::open_in_memory();
    let mut state = AppState::for_tests(conn);
    state.environment = artha_server::config::Environment::Production;
    state.paddle_ocr_cmd = None;
    let db_ref = state.db.clone();
    let app = build_app(state);
    let token = setup_owner(&app).await;

    // Create a product
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/products",
            Some(&token),
            json!({ "name": "Cooking Oil 1L", "qr_code": "QR-OIL-1", "price_paise": 15000, "cost_paise": 12000, "gst_rate_bps": 500 }),
        ))
        .await
        .unwrap();
    let oil_id = body_json(resp).await["product_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Create draft purchase & upload bill image
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    let valid_png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill.png",
            valid_png,
            "image/png",
        ))
        .await
        .unwrap();
    let image_id = body_json(resp).await["image_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Trigger OCR in production mode with no command configured -> must fail clearly
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/ocr"),
            Some(&token),
            json!({ "image_id": image_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err_json = body_json(resp).await;
    let msg = err_json["message"].as_str().unwrap();
    assert!(
        msg.contains("Local OCR engine is not configured on this server"),
        "Must return actionable error, got: {msg}"
    );

    // Verify invariants: draft unchanged, no OCR results recorded, stock unchanged
    {
        let conn = db_ref.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM purchases WHERE purchase_id = ?1",
                rusqlite::params![purchase_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "draft");

        let ocr_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ocr_results WHERE image_id = ?1",
                rusqlite::params![image_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ocr_count, 0, "No OCR results should be written on failure");

        let stock: i64 = conn
            .query_row(
                "SELECT stock_qty_milli FROM products WHERE product_id = ?1",
                rusqlite::params![oil_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stock, 0, "Stock must remain 0 on OCR failure");
    }

    // Verify core purchase workflow still functions: manual entry and approval
    let resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/purchases/{purchase_id}"),
            Some(&token),
            json!({
                "invoice_no": "MANUAL-INV-001",
                "items": [{
                    "product_id": oil_id,
                    "product_name_snapshot": "Cooking Oil 1L",
                    "quantity_milli": 10000,
                    "price_paise": 12000,
                    "gst_rate_bps": 500,
                }],
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/approve"),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Stock successfully updated to 10000 milli (10 units)
    let conn = db_ref.lock().unwrap();
    let stock: i64 = conn
        .query_row(
            "SELECT stock_qty_milli FROM products WHERE product_id = ?1",
            rusqlite::params![oil_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stock, 10000);
}

#[tokio::test]
async fn test_ocr_failure_and_timeout_preserve_draft_and_inventory() {
    let conn = db::open_in_memory();
    let mut state = AppState::for_tests(conn);
    // 1. Configured with a non-existent command
    state.paddle_ocr_cmd = Some("nonexistent_ocr_command_binary_12345".to_string());
    let db_ref = state.db.clone();
    let app = build_app(state);
    let token = setup_owner(&app).await;

    // Create draft purchase & upload image
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    let valid_png: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    let resp = app
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id}/image"),
            &token,
            "bill.png",
            valid_png,
            "image/png",
        ))
        .await
        .unwrap();
    let image_id = body_json(resp).await["image_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Trigger OCR with non-existent command -> 400 Bad Request
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id}/ocr"),
            Some(&token),
            json!({ "image_id": image_id }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Verify purchase draft status is still "draft"
    {
        let conn = db_ref.lock().unwrap();
        let status: String = conn
            .query_row(
                "SELECT status FROM purchases WHERE purchase_id = ?1",
                rusqlite::params![purchase_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "draft");
    }

    // 2. Timeout test
    let temp_dir = std::env::temp_dir();
    #[cfg(windows)]
    let (script_path, hang_cmd) = {
        let script = temp_dir.join(format!("test_hang_ocr_route_{}.cmd", std::process::id()));
        std::fs::write(&script, "@ping -n 5 127.0.0.1 >nul\r\n").unwrap();
        let path_str = script.to_string_lossy().to_string();
        (Some(script), path_str)
    };
    #[cfg(not(windows))]
    let (script_path, hang_cmd): (Option<std::path::PathBuf>, String) = (None, "sleep".to_string());

    let conn2 = db::open_in_memory();
    let mut state2 = AppState::for_tests(conn2);
    state2.paddle_ocr_cmd = Some(hang_cmd);
    state2.ocr_timeout_secs = 1;
    let app2 = build_app(state2);
    let token2 = setup_owner(&app2).await;

    let resp = app2
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/purchases",
            Some(&token2),
            json!({ "source": "ocr" }),
        ))
        .await
        .unwrap();
    let purchase_id2 = body_json(resp).await["purchase_id"]
        .as_str()
        .unwrap()
        .to_string();

    let valid_png2: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52,
    ];
    let resp = app2
        .clone()
        .oneshot(multipart_image_request(
            &format!("/api/purchases/{purchase_id2}/image"),
            &token2,
            "bill.png",
            valid_png2,
            "image/png",
        ))
        .await
        .unwrap();
    let image_id2 = body_json(resp).await["image_id"]
        .as_str()
        .unwrap()
        .to_string();

    let resp = app2
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/purchases/{purchase_id2}/ocr"),
            Some(&token2),
            json!({ "image_id": image_id2 }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err_msg = body_json(resp).await["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        err_msg.contains("timed out"),
        "Expected timeout error, got: {err_msg}"
    );

    if let Some(p) = script_path {
        let _ = std::fs::remove_file(p);
    }
}
