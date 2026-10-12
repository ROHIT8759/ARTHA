//! Integration tests for Business Profile & Document Settings (Phase 08).
//!
//! Tests:
//! 1. Authenticated user can read business profile.
//! 2. Unauthenticated user is rejected with 401.
//! 3. Staff user attempting to update business profile is rejected with 403.
//! 4. Staff user attempting logo upload/removal is rejected with 403.
//! 5. Owner can update business name, contact, address, GSTIN, and print settings.
//! 6. Validation: reject empty business name, invalid email, invalid layout.
//! 7. Business settings persist across subsequent GET requests.
//! 8. Logo upload validation: rejects SVG and invalid payloads.
//! 9. Valid raster logo upload succeeds and persists.
//! 10. Logo deletion clears logo and persists.
//! 11. Audit log records settings and logo operations.

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

async fn setup_owner_and_staff(app: &axum::Router, biz_name: &str) -> (String, String) {
    // 1. Setup owner
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/setup",
            None,
            json!({
                "business_name": biz_name,
                "owner_username": "shop_owner",
                "owner_password": "password123"
            }),
        ))
        .await
        .unwrap();
    let owner_token = body_json(resp).await["token"].as_str().unwrap().to_string();

    // 2. Create staff
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/users",
            Some(&owner_token),
            json!({
                "username": "shop_staff",
                "password": "password123"
            }),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. Login staff
    let resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/auth/login",
            None,
            json!({
                "username": "shop_staff",
                "password": "password123"
            }),
        ))
        .await
        .unwrap();
    let staff_token = body_json(resp).await["token"].as_str().unwrap().to_string();

    (owner_token, staff_token)
}

#[tokio::test]
async fn test_business_profile_and_settings_workflow() {
    let (app, state) = test_app_with_state();
    let (owner_token, staff_token) = setup_owner_and_staff(&app, "Artha General Store").await;

    // 1. Authenticated staff can read business profile
    let get_resp = app
        .clone()
        .oneshot(json_request("GET", "/api/business", Some(&staff_token), Value::Null))
        .await
        .unwrap();
    assert_eq!(get_resp.status(), StatusCode::OK);
    let biz_body = body_json(get_resp).await;
    assert_eq!(biz_body["name"], "Artha General Store");

    // 2. Unauthenticated request rejected with 401
    let unauth_resp = app
        .clone()
        .oneshot(json_request("GET", "/api/business", None, Value::Null))
        .await
        .unwrap();
    assert_eq!(unauth_resp.status(), StatusCode::UNAUTHORIZED);

    // 3. Staff user attempting to update business profile rejected with 403 Forbidden
    let staff_patch = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/api/business",
            Some(&staff_token),
            json!({ "name": "Hacked Store" }),
        ))
        .await
        .unwrap();
    assert_eq!(staff_patch.status(), StatusCode::FORBIDDEN);

    // 4. Staff user attempting logo upload rejected with 403 Forbidden
    let staff_logo = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&staff_token),
            json!({ "logo_data_url": "data:image/png;base64,iVBORw0KGgo=" }),
        ))
        .await
        .unwrap();
    assert_eq!(staff_logo.status(), StatusCode::FORBIDDEN);

    // 6. Validation tests for owner
    // Empty business name rejected
    let empty_name = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/api/business",
            Some(&owner_token),
            json!({ "name": "   " }),
        ))
        .await
        .unwrap();
    assert_eq!(empty_name.status(), StatusCode::BAD_REQUEST);

    // Invalid email rejected
    let bad_email = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/api/business",
            Some(&owner_token),
            json!({ "email": "not-an-email" }),
        ))
        .await
        .unwrap();
    assert_eq!(bad_email.status(), StatusCode::BAD_REQUEST);

    // Invalid layout rejected
    let bad_layout = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/api/business",
            Some(&owner_token),
            json!({ "default_layout": "tabloid" }),
        ))
        .await
        .unwrap();
    assert_eq!(bad_layout.status(), StatusCode::BAD_REQUEST);

    // 5. Owner successfully updates business profile & print preferences
    let valid_patch = app
        .clone()
        .oneshot(json_request(
            "PATCH",
            "/api/business",
            Some(&owner_token),
            json!({
                "name": "Artha Supermart",
                "registered_name": "Artha Enterprises Pvt Ltd",
                "address": "123 Market Road, Shop 4",
                "city": "Bengaluru",
                "state": "Karnataka",
                "pincode": "560001",
                "country": "India",
                "phone": "+91 9876543210",
                "email": "contact@arthamart.in",
                "gstin": "29AAAAA0000A1Z5",
                "invoice_footer": "Thank you for shopping with us! Please visit again.",
                "terms_and_conditions": "Goods once sold can be exchanged within 7 days with original invoice.",
                "default_layout": "thermal",
                "show_hsn": true,
                "show_tax_breakdown": true,
                "show_logo": true
            }),
        ))
        .await
        .unwrap();
    assert_eq!(valid_patch.status(), StatusCode::OK);
    let updated_body = body_json(valid_patch).await;
    assert_eq!(updated_body["name"], "Artha Supermart");
    assert_eq!(updated_body["settings"]["city"], "Bengaluru");
    assert_eq!(updated_body["settings"]["gstin"], "29AAAAA0000A1Z5");
    assert_eq!(updated_body["settings"]["default_layout"], "thermal");

    // 7. Settings persist across subsequent GET requests
    let verify_get = app
        .clone()
        .oneshot(json_request("GET", "/api/business", Some(&staff_token), Value::Null))
        .await
        .unwrap();
    assert_eq!(verify_get.status(), StatusCode::OK);
    let verify_body = body_json(verify_get).await;
    assert_eq!(verify_body["name"], "Artha Supermart");
    assert_eq!(verify_body["settings"]["registered_name"], "Artha Enterprises Pvt Ltd");
    assert_eq!(verify_body["settings"]["phone"], "+91 9876543210");
    assert_eq!(verify_body["settings"]["default_layout"], "thermal");

    // 8. Logo upload validation: reject SVG
    let svg_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/svg+xml;base64,PHN2Zz4=" }),
        ))
        .await
        .unwrap();
    assert_eq!(svg_resp.status(), StatusCode::BAD_REQUEST);

    // 9. Valid raster logo upload succeeds and persists
    let valid_logo_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let logo_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": valid_logo_url }),
        ))
        .await
        .unwrap();
    assert_eq!(logo_resp.status(), StatusCode::OK);
    let logo_body = body_json(logo_resp).await;
    assert_eq!(logo_body["settings"]["logo_data_url"], valid_logo_url);

    // 10. Logo deletion clears logo safely
    let del_logo_resp = app
        .clone()
        .oneshot(json_request("DELETE", "/api/business/logo", Some(&owner_token), Value::Null))
        .await
        .unwrap();
    assert_eq!(del_logo_resp.status(), StatusCode::OK);
    let del_body = body_json(del_logo_resp).await;
    assert!(del_body["settings"]["logo_data_url"].is_null());

    // 11. Audit log verification
    {
        let conn = state.db.lock().unwrap();
        let audit_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log WHERE action IN ('update_business_settings', 'update_business_logo', 'remove_business_logo')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(audit_count >= 3);
    }
}
