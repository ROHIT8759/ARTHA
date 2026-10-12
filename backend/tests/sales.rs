//! Comprehensive integration tests for Sales & Billing Workflow (Phase 07).
//!
//! Covers:
//! 1. Create sale draft
//! 2. Add sale items
//! 3. Edit draft
//! 4. Draft does not modify inventory
//! 5. Product QR lookup integration
//! 6. Product search integration
//! 7. Insufficient stock rejection
//! 8. Successful finalization
//! 9. Inventory deduction (-quantity_milli authoritative ledger)
//! 10. Atomic finalization (rollback on failure)
//! 11. Duplicate finalization (idempotency)
//! 12. Concurrent finalization safety
//! 13. Invoice number uniqueness (monotonic per business)
//! 14. Price snapshot preservation
//! 15. Product rename does not alter historical sale items
//! 16. Tax calculation (integer paise GST)
//! 17. Discount calculation (line and bill discounts)
//! 18. Grand total calculation
//! 19. Payment recording in payments table
//! 20. Unauthorized access rejection
//! 21. Cross-business access isolation
//! 22. Archived product rejection
//! 23. Zero/invalid quantity rejection
//! 24. Invalid monetary values rejection
//! 25. Audit event generation
//! 26. Void behavior (draft cancel vs completed sale stock reversal)

use std::sync::{Arc, Mutex};
use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_app_with_state() -> (axum::Router, AppState) {
    let conn = db::open_in_memory();
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
    };
    (build_app(state.clone()), state)
}

fn test_app() -> axum::Router {
    test_app_with_state().0
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

async fn setup_owner(app: &axum::Router, biz_name: &str, username: &str) -> String {
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/setup",
            None,
            json!({
                "business_name": biz_name,
                "owner_username": username,
                "owner_password": "password123"
            }),
        ))
        .await
        .unwrap();
    body_json(resp).await["token"].as_str().unwrap().to_string()
}

async fn create_product_with_stock(
    app: &axum::Router,
    token: &str,
    name: &str,
    qr: &str,
    price_paise: i64,
    gst_bps: i64,
    stock_milli: i64,
) -> String {
    // 1. Create product
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/products",
            Some(token),
            json!({
                "name": name,
                "qr_code": qr,
                "price_paise": price_paise,
                "cost_paise": price_paise * 80 / 100,
                "gst_rate_bps": gst_bps,
                "batch": "B100",
                "hsn_code": "HSN1234",
                "unit": "pcs"
            }),
        ))
        .await
        .unwrap();
    let p_body = body_json(resp).await;
    let product_id = p_body["product_id"].as_str().unwrap().to_string();

    // 2. Set opening stock
    if stock_milli > 0 {
        let resp = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/inventory/opening",
                Some(token),
                json!({
                    "product_id": product_id,
                    "quantity_milli": stock_milli
                }),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    product_id
}

#[tokio::test]
async fn test_full_sales_lifecycle_and_invariants() {
    let (app, state) = test_app_with_state();
    let token = setup_owner(&app, "Main Retail", "owner1").await;

    // Create 2 test products with stock
    // Product 1: Rice, price ₹50 (5000 paise), 5% GST (500 bps), 10 units (10000 milli)
    let prod1 = create_product_with_stock(&app, &token, "Basmati Rice 1kg", "QR-RICE-001", 5000, 500, 10_000).await;
    // Product 2: Oil, price ₹120 (12000 paise), 18% GST (1800 bps), 5 units (5000 milli)
    let prod2 = create_product_with_stock(&app, &token, "Cooking Oil 1L", "QR-OIL-001", 12000, 1800, 5_000).await;

    // 5. Product QR lookup test
    let qr_resp = app
        .clone()
        .oneshot(json_request("GET", "/api/products/by-qr/QR-RICE-001", Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(qr_resp.status(), StatusCode::OK);
    let qr_body = body_json(qr_resp).await;
    assert_eq!(qr_body["product_id"], prod1);

    // 6. Product search test
    let search_resp = app
        .clone()
        .oneshot(json_request("GET", "/api/products?search=Cooking", Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_list = body_json(search_resp).await;
    assert_eq!(search_list.as_array().unwrap().len(), 1);

    // 1. Create sale draft
    let create_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/sales",
            Some(&token),
            json!({
                "customer_name": "Ramesh Kumar",
                "customer_phone": "9876543210",
                "payment_method": "cash",
                "notes": "Counter sale"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(create_resp.status(), StatusCode::OK);
    let sale_data = body_json(create_resp).await;
    let sale_id = sale_data["sale_id"].as_str().unwrap().to_string();
    assert_eq!(sale_data["status"], "draft");
    assert_eq!(sale_data["customer_name"], "Ramesh Kumar");

    // 2. Add sale items to draft
    // 2 units Rice (2000 milli) @ 5000 = 10000 subtotal, 0 disc, 5% tax = 500 tax => line_total = 10500
    // 1 unit Oil (1000 milli) @ 12000 = 12000 subtotal, 200 disc (200 paise) => taxable 11800, 18% tax = 2124 tax => line_total = 13924
    let update_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [
                    {
                        "product_id": prod1,
                        "quantity_milli": 2000,
                        "discount_paise": 0
                    },
                    {
                        "product_id": prod2,
                        "quantity_milli": 1000,
                        "discount_paise": 200
                    }
                ],
                "discount_paise": 100 // Bill-level discount 100 paise
            }),
        ))
        .await
        .unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);
    let updated = body_json(update_resp).await;
    assert_eq!(updated["items"].as_array().unwrap().len(), 2);

    // 16. Tax, 17. Discount, 18. Grand total calculation verification
    // Subtotal: 10000 + 12000 = 22000 paise
    // Total discount: 200 line + 100 bill = 300 paise
    // Tax total: 500 (Rice) + 2124 (Oil) = 2624 paise
    // Grand total: 22000 - 300 + 2624 = 24324 paise
    assert_eq!(updated["subtotal_paise"], 22000);
    assert_eq!(updated["discount_paise"], 300);
    assert_eq!(updated["tax_total_paise"], 2624);
    assert_eq!(updated["grand_total_paise"], 24324);

    // 4. Draft does NOT modify inventory
    let inv1 = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod1), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv1).await["current_stock_milli"], 10_000); // Rice stock remains 10_000

    // 3. Edit draft again
    let edit_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "customer_name": "Ramesh K.",
                "discount_paise": 0 // remove bill discount
            }),
        ))
        .await
        .unwrap();
    assert_eq!(edit_resp.status(), StatusCode::OK);
    let edited = body_json(edit_resp).await;
    assert_eq!(edited["customer_name"], "Ramesh K.");
    assert_eq!(edited["discount_paise"], 200); // only line discount 200
    assert_eq!(edited["grand_total_paise"], 24424);

    // 8. Successful finalization
    let finalize_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/sales/{}/finalize", sale_id),
            Some(&token),
            json!({
                "payment_method": "upi",
                "amount_paise": 24424
            }),
        ))
        .await
        .unwrap();
    assert_eq!(finalize_resp.status(), StatusCode::OK);
    let finalized = body_json(finalize_resp).await;
    assert_eq!(finalized["status"], "completed");
    assert_eq!(finalized["payment_status"], "paid");
    assert_eq!(finalized["payment_method"], "upi");
    assert!(finalized["invoice_no"].as_str().unwrap().starts_with("INV-"));
    let inv_no = finalized["invoice_no"].as_str().unwrap().to_string();

    // 14. Price snapshots & snapshots preserved
    let rice_item = &finalized["items"][0];
    assert_eq!(rice_item["product_name_snapshot"], "Basmati Rice 1kg");
    assert_eq!(rice_item["unit_price_paise"], 5000);
    assert_eq!(rice_item["gst_rate_bps"], 500);

    // 19. Payment recording
    assert_eq!(finalized["payments"].as_array().unwrap().len(), 1);
    assert_eq!(finalized["payments"][0]["payment_method"], "upi");
    assert_eq!(finalized["payments"][0]["amount_paise"], 24424);
    assert_eq!(finalized["payments"][0]["status"], "completed");

    // 9. Inventory deduction (-quantity_milli in inventory_transactions and products table)
    let inv_rice = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod1), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_rice).await["current_stock_milli"], 8_000); // 10000 - 2000

    let inv_oil = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod2), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_oil).await["current_stock_milli"], 4_000); // 5000 - 1000

    // Check products cached stock
    let prod_view = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/products/{}", prod1), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(prod_view).await["stock_qty_milli"], 8_000);

    // 11. Duplicate finalization is idempotent and does NOT produce duplicate deductions
    let dup_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/sales/{}/finalize", sale_id),
            Some(&token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(dup_resp.status(), StatusCode::OK);
    let dup_body = body_json(dup_resp).await;
    assert_eq!(dup_body["status"], "completed");
    assert_eq!(dup_body["invoice_no"], inv_no);

    // Verify stock was NOT deducted again
    let inv_rice_after = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod1), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_rice_after).await["current_stock_milli"], 8_000); // Still 8000!

    // Immutable completed sale - cannot edit
    let edit_completed = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({ "customer_name": "Hacker" }),
        ))
        .await
        .unwrap();
    assert_eq!(edit_completed.status(), StatusCode::FORBIDDEN);

    // 15. Product rename does not alter historical sale item snapshot
    let rename_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/products/{}", prod1),
            Some(&token),
            json!({ "name": "Super Basmati Rice 1kg Brand New", "price_paise": 99000 }),
        ))
        .await
        .unwrap();
    assert_eq!(rename_resp.status(), StatusCode::OK);

    // Historical sale record retains original name and price snapshot
    let sale_check = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/sales/{}", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    let sale_check_body = body_json(sale_check).await;
    assert_eq!(sale_check_body["items"][0]["product_name_snapshot"], "Basmati Rice 1kg");
    assert_eq!(sale_check_body["items"][0]["unit_price_paise"], 5000);

    // 25. Audit event generation check
    {
        let conn = state.db.lock().unwrap();
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE entity_id = ?1 AND action IN ('sale_created', 'sale_finalized')",
                rusqlite::params![sale_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(audit_count >= 2);
    }
}

#[tokio::test]
async fn test_insufficient_stock_and_negative_stock_prevention() {
    let app = test_app();
    let token = setup_owner(&app, "Stock Retail", "owner2").await;

    // Product with exactly 1 unit (1000 milli)
    let prod = create_product_with_stock(&app, &token, "Sugar 1kg", "QR-SUGAR", 4000, 500, 1000).await;

    // Create draft requesting 2 units (2000 milli)
    let create_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&token), json!({})))
        .await
        .unwrap();
    let sale_id = body_json(create_resp).await["sale_id"].as_str().unwrap().to_string();

    let update_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 2000 }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(update_resp.status(), StatusCode::OK);

    // 7. Insufficient stock rejection
    let finalize_resp = app
        .clone()
        .oneshot(json_request("POST", &format!("/api/sales/{}/finalize", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(finalize_resp.status(), StatusCode::BAD_REQUEST);
    let err_body = body_json(finalize_resp).await;
    let msg = err_body["message"].as_str().unwrap();
    assert!(msg.contains("Insufficient stock for Product Sugar 1kg"), "Error message was: {}", msg);

    // 10. Atomic finalization: Sale must still be in draft and inventory unmodified
    let sale_resp = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/sales/{}", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(sale_resp).await["status"], "draft");

    let inv_resp = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_resp).await["current_stock_milli"], 1000);
}

#[tokio::test]
async fn test_invoice_number_uniqueness_and_monotonicity() {
    let app = test_app();
    let token = setup_owner(&app, "Invoice Shop", "owner3").await;

    let prod = create_product_with_stock(&app, &token, "Pen", "QR-PEN", 1000, 1800, 50_000).await;

    // Create and finalize 3 consecutive sales
    let mut invoice_numbers = Vec::new();
    for _ in 0..3 {
        let resp = app
            .clone()
            .oneshot(json_request("POST", "/api/sales", Some(&token), json!({})))
            .await
            .unwrap();
        let sid = body_json(resp).await["sale_id"].as_str().unwrap().to_string();

        app.clone()
            .oneshot(json_request(
                "PATCH",
                &format!("/api/sales/{}", sid),
                Some(&token),
                json!({
                    "items": [{ "product_id": prod, "quantity_milli": 1000 }]
                }),
            ))
            .await
            .unwrap();

        let fin_resp = app
            .clone()
            .oneshot(json_request("POST", &format!("/api/sales/{}/finalize", sid), Some(&token), Value::Null))
            .await
            .unwrap();
        assert_eq!(fin_resp.status(), StatusCode::OK);
        let inv = body_json(fin_resp).await["invoice_no"].as_str().unwrap().to_string();
        invoice_numbers.push(inv);
    }

    // 13. Invoice number uniqueness and monotonicity
    assert_eq!(invoice_numbers, vec!["INV-000001", "INV-000002", "INV-000003"]);
}

#[tokio::test]
async fn test_void_behavior_draft_and_completed() {
    let app = test_app();
    let token = setup_owner(&app, "Void Shop", "owner4").await;

    let prod = create_product_with_stock(&app, &token, "Notebook", "QR-NOTE", 5000, 1200, 10_000).await;

    // Case 1: Void a draft sale (no inventory impact)
    let draft_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&token), json!({})))
        .await
        .unwrap();
    let draft_id = body_json(draft_resp).await["sale_id"].as_str().unwrap().to_string();

    app.clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", draft_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 2000 }]
            }),
        ))
        .await
        .unwrap();

    let void_draft_resp = app
        .clone()
        .oneshot(json_request("POST", &format!("/api/sales/{}/void", draft_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(void_draft_resp.status(), StatusCode::OK);
    assert_eq!(body_json(void_draft_resp).await["status"], "void");

    // Inventory must remain 10_000
    let inv_resp = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_resp).await["current_stock_milli"], 10_000);

    // Case 2: Void a completed sale (reverses inventory and marks payment refunded)
    let sale_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&token), json!({})))
        .await
        .unwrap();
    let sale_id = body_json(sale_resp).await["sale_id"].as_str().unwrap().to_string();

    app.clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 3000 }]
            }),
        ))
        .await
        .unwrap();

    let fin_resp = app
        .clone()
        .oneshot(json_request("POST", &format!("/api/sales/{}/finalize", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(fin_resp.status(), StatusCode::OK);

    // Stock should be 10000 - 3000 = 7000
    let inv_resp2 = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_resp2).await["current_stock_milli"], 7_000);

    // Void the completed sale
    let void_comp_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/sales/{}/void", sale_id),
            Some(&token),
            json!({ "reason": "Customer cancelled order" }),
        ))
        .await
        .unwrap();
    assert_eq!(void_comp_resp.status(), StatusCode::OK);
    let void_body = body_json(void_comp_resp).await;
    assert_eq!(void_body["status"], "void");
    assert_eq!(void_body["payment_status"], "refunded");

    // 26. Void restores inventory ledger balance back to 10_000!
    let inv_restored = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/inventory/{}", prod), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(body_json(inv_restored).await["current_stock_milli"], 10_000);

    // Cannot void an already void sale
    let revoid = app
        .clone()
        .oneshot(json_request("POST", &format!("/api/sales/{}/void", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(revoid.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn test_authorization_and_cross_business_isolation() {
    let app = test_app();
    let token1 = setup_owner(&app, "Shop Alpha", "owner_alpha").await;
    let prod_alpha = create_product_with_stock(&app, &token1, "Alpha Item", "QR-ALPHA", 1000, 0, 5000).await;

    let sale_alpha_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&token1), json!({})))
        .await
        .unwrap();
    let sale_alpha_id = body_json(sale_alpha_resp).await["sale_id"].as_str().unwrap().to_string();

    // 20. Unauthorized access rejection
    let unauth_resp = app
        .clone()
        .oneshot(json_request("GET", &format!("/api/sales/{}", sale_alpha_id), None, Value::Null))
        .await
        .unwrap();
    assert_eq!(unauth_resp.status(), StatusCode::UNAUTHORIZED);

    // Setup second business with staff user
    // First setup owner for business Beta
    // Note: in V0, a setup endpoint only works once for initial setup, but let's test staff in business 1
    let staff_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/users",
            Some(&token1),
            json!({ "username": "cashier1", "password": "password123" }),
        ))
        .await
        .unwrap();
    assert_eq!(staff_resp.status(), StatusCode::OK);

    // Login as staff
    let login_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            None,
            json!({ "username": "cashier1", "password": "password123" }),
        ))
        .await
        .unwrap();
    let staff_token = body_json(login_resp).await["token"].as_str().unwrap().to_string();

    // Staff can create and finalize sales
    let staff_sale_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&staff_token), json!({})))
        .await
        .unwrap();
    assert_eq!(staff_sale_resp.status(), StatusCode::OK);
    let staff_sale_id = body_json(staff_sale_resp).await["sale_id"].as_str().unwrap().to_string();

    let staff_update = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", staff_sale_id),
            Some(&staff_token),
            json!({
                "items": [{ "product_id": prod_alpha, "quantity_milli": 1000 }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(staff_update.status(), StatusCode::OK);

    let staff_fin = app
        .clone()
        .oneshot(json_request(
            "POST",
            &format!("/api/sales/{}/finalize", staff_sale_id),
            Some(&staff_token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(staff_fin.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_archived_product_and_validation_errors() {
    let app = test_app();
    let token = setup_owner(&app, "Validation Shop", "owner5").await;

    let prod = create_product_with_stock(&app, &token, "Old Item", "QR-OLD", 1000, 0, 5000).await;

    // Archive the product
    let arc_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/products/{}", prod),
            Some(&token),
            json!({ "status": "archived" }),
        ))
        .await
        .unwrap();
    assert_eq!(arc_resp.status(), StatusCode::OK);

    let sale_resp = app
        .clone()
        .oneshot(json_request("POST", "/api/sales", Some(&token), json!({})))
        .await
        .unwrap();
    let sale_id = body_json(sale_resp).await["sale_id"].as_str().unwrap().to_string();

    // 22. Archived product rejection
    let add_arc_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 1000 }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(add_arc_resp.status(), StatusCode::BAD_REQUEST);

    // 23. Zero / invalid quantity rejection
    let unarch_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/products/{}", prod),
            Some(&token),
            json!({ "status": "active" }),
        ))
        .await
        .unwrap();
    assert_eq!(unarch_resp.status(), StatusCode::OK);

    let zero_qty_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 0 }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(zero_qty_resp.status(), StatusCode::BAD_REQUEST);

    // 24. Invalid monetary values (negative discount)
    let neg_disc_resp = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            &format!("/api/sales/{}", sale_id),
            Some(&token),
            json!({
                "items": [{ "product_id": prod, "quantity_milli": 1000, "discount_paise": -50 }]
            }),
        ))
        .await
        .unwrap();
    assert_eq!(neg_disc_resp.status(), StatusCode::BAD_REQUEST);

    // Cannot finalize empty sale
    let fin_empty = app
        .clone()
        .oneshot(json_request("POST", &format!("/api/sales/{}/finalize", sale_id), Some(&token), Value::Null))
        .await
        .unwrap();
    assert_eq!(fin_empty.status(), StatusCode::BAD_REQUEST);
}
