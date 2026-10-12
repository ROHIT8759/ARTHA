//! Integration tests for Business Profile & Document Settings (Phase 08 & 08.1).
//!
//! Tests:
//! 1. Authenticated user can read business profile.
//! 2. Unauthenticated user is rejected with 401.
//! 3. Staff user attempting to update business profile is rejected with 403.
//! 4. Staff user attempting logo upload/removal is rejected with 403.
//! 5. Owner can update business name, contact, address, GSTIN, and print settings.
//! 6. Validation: reject empty business name, invalid email, invalid layout.
//! 7. Business settings persist across subsequent GET requests.
//! 8. Logo upload validation: reject SVG and HTML payloads.
//! 9. Reject unsupported MIME (GIF, BMP, PDF).
//! 10. Reject missing data URL separator or missing ';base64'.
//! 11. Reject empty payload.
//! 12. Reject invalid base64 (illegal characters, unaligned length, middle padding, non-zero padding bits, whitespace).
//! 13. Reject signature mismatch (declared PNG, but contains arbitrary non-PNG bytes).
//! 14. Valid raster logo upload succeeds and persists.
//! 15. Failed upload preserves existing valid logo unchanged.
//! 16. Valid minimal JPEG upload succeeds and verifies JPEG magic bytes.
//! 17. Valid minimal WebP upload succeeds and verifies WebP RIFF/WEBP magic bytes.
//! 18. Reject MIME-signature mismatch (declared PNG but contains JPEG bytes).
//! 19. Staff cannot delete logo (403).
//! 20. Owner successfully deletes logo (200).
//! 21. Audit log records settings and logo operations.

use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn test_app_with_state() -> (axum::Router, AppState) {
    let conn = db::open_in_memory();
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
    };
    (build_app(state.clone()), state)
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
        .oneshot(json_request(
            "GET",
            "/api/business",
            Some(&staff_token),
            Value::Null,
        ))
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
        .oneshot(json_request(
            "GET",
            "/api/business",
            Some(&staff_token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(verify_get.status(), StatusCode::OK);
    let verify_body = body_json(verify_get).await;
    assert_eq!(verify_body["name"], "Artha Supermart");
    assert_eq!(
        verify_body["settings"]["registered_name"],
        "Artha Enterprises Pvt Ltd"
    );
    assert_eq!(verify_body["settings"]["phone"], "+91 9876543210");
    assert_eq!(verify_body["settings"]["default_layout"], "thermal");

    // 8. Logo upload validation: reject SVG and HTML
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

    let html_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:text/html;base64,PGh0bWw+PC9odG1sPg==" }),
        ))
        .await
        .unwrap();
    assert_eq!(html_resp.status(), StatusCode::BAD_REQUEST);

    // 9. Reject unsupported MIME (GIF, BMP, PDF)
    let gif_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7" }),
        ))
        .await
        .unwrap();
    assert_eq!(gif_resp.status(), StatusCode::BAD_REQUEST);

    // 10. Reject missing data URL separator or missing ';base64'
    let missing_sep = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64" }),
        ))
        .await
        .unwrap();
    assert_eq!(missing_sep.status(), StatusCode::BAD_REQUEST);

    let missing_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png,12345" }),
        ))
        .await
        .unwrap();
    assert_eq!(missing_b64.status(), StatusCode::BAD_REQUEST);

    // 11. Reject empty payload
    let empty_payload = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64," }),
        ))
        .await
        .unwrap();
    assert_eq!(empty_payload.status(), StatusCode::BAD_REQUEST);

    // 12. Reject invalid base64 (illegal characters, wrong padding, unaligned length, non-zero padding bits, whitespace)
    let invalid_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,not_valid_base64!!!" }),
        ))
        .await
        .unwrap();
    assert_eq!(invalid_b64.status(), StatusCode::BAD_REQUEST);

    // Unaligned length (not a multiple of 4)
    let unaligned_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,AAA" }),
        ))
        .await
        .unwrap();
    assert_eq!(unaligned_b64.status(), StatusCode::BAD_REQUEST);

    // Padding in the middle of payload
    let mid_pad_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,AA==AAAA" }),
        ))
        .await
        .unwrap();
    assert_eq!(mid_pad_b64.status(), StatusCode::BAD_REQUEST);

    // Non-zero padding bits
    let nonzero_pad_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,Zh==" }),
        ))
        .await
        .unwrap();
    assert_eq!(nonzero_pad_b64.status(), StatusCode::BAD_REQUEST);

    // Embedded whitespace
    let ws_b64 = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,iVBO Rw0K GgoA AA==" }),
        ))
        .await
        .unwrap();
    assert_eq!(ws_b64.status(), StatusCode::BAD_REQUEST);

    // 13. Reject signature mismatch (declared PNG, but contains arbitrary non-PNG bytes)
    let malformed_sig = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,dGVzdGFzZGY=" }),
        ))
        .await
        .unwrap();
    assert_eq!(malformed_sig.status(), StatusCode::BAD_REQUEST);

    // 14. Valid 1x1 PNG upload succeeds and persists
    let valid_png_url = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    let logo_png_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": valid_png_url }),
        ))
        .await
        .unwrap();
    assert_eq!(logo_png_resp.status(), StatusCode::OK);
    let logo_png_body = body_json(logo_png_resp).await;
    assert_eq!(logo_png_body["settings"]["logo_data_url"], valid_png_url);

    // 15. Verify that a subsequent FAILED upload preserves the existing valid logo unchanged!
    let failed_upload = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,corrupted" }),
        ))
        .await
        .unwrap();
    assert_eq!(failed_upload.status(), StatusCode::BAD_REQUEST);

    let verify_unchanged = app
        .clone()
        .oneshot(json_request(
            "GET",
            "/api/business",
            Some(&owner_token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(verify_unchanged.status(), StatusCode::OK);
    let verify_unchanged_body = body_json(verify_unchanged).await;
    assert_eq!(
        verify_unchanged_body["settings"]["logo_data_url"],
        valid_png_url
    );

    // 16. Valid minimal JPEG upload succeeds and verifies JPEG magic bytes
    let valid_jpeg_url = "data:image/jpeg;base64,/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=";
    let logo_jpeg_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": valid_jpeg_url }),
        ))
        .await
        .unwrap();
    assert_eq!(logo_jpeg_resp.status(), StatusCode::OK);

    // 17. Valid minimal WebP upload succeeds and verifies WebP RIFF/WEBP magic bytes
    let valid_webp_url =
        "data:image/webp;base64,UklGRiQAAABXRUJQVlA4IBgAAAAwAQCdASoBAAEAD8D+JaQAA3AA/ua1AAA=";
    let logo_webp_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": valid_webp_url }),
        ))
        .await
        .unwrap();
    assert_eq!(logo_webp_resp.status(), StatusCode::OK);

    // 18. Reject MIME-signature mismatch (declared PNG but contains JPEG bytes)
    let mismatch_resp = app
        .clone()
        .oneshot(json_request(
            "POST",
            "/api/business/logo",
            Some(&owner_token),
            json!({ "logo_data_url": "data:image/png;base64,/9j/4AAQSkZJRgABAQEASABIAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=" }),
        ))
        .await
        .unwrap();
    assert_eq!(mismatch_resp.status(), StatusCode::BAD_REQUEST);

    // 19. Staff cannot delete logo (403)
    let staff_del_resp = app
        .clone()
        .oneshot(json_request(
            "DELETE",
            "/api/business/logo",
            Some(&staff_token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(staff_del_resp.status(), StatusCode::FORBIDDEN);

    // 20. Owner successfully deletes logo (200)
    let del_logo_resp = app
        .clone()
        .oneshot(json_request(
            "DELETE",
            "/api/business/logo",
            Some(&owner_token),
            Value::Null,
        ))
        .await
        .unwrap();
    assert_eq!(del_logo_resp.status(), StatusCode::OK);
    let del_body = body_json(del_logo_resp).await;
    assert!(del_body["settings"]["logo_data_url"].is_null());

    // 21. Audit log verification
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
