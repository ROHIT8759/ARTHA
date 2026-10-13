use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::path::Path as StdPath;
use tokio::fs;

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::AppState;

pub const MAX_BILL_IMAGE_BYTES: usize = 10 * 1024 * 1024; // 10 MiB

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/purchases", get(list_purchases).post(create_purchase))
        .route(
            "/api/purchases/:id",
            get(get_purchase).patch(update_purchase),
        )
        .route("/api/purchases/:id/approve", post(approve_purchase))
        .route("/api/purchases/:id/cancel", post(cancel_purchase))
        .route(
            "/api/purchases/:id/image",
            get(get_bill_image)
                .post(upload_bill_image)
                .layer(DefaultBodyLimit::max(MAX_BILL_IMAGE_BYTES)),
        )
        .route("/api/purchases/:id/ocr", post(process_ocr))
}

#[derive(Debug, Serialize)]
pub struct PurchaseItemView {
    pub purchase_item_id: String,
    pub product_id: Option<String>,
    pub product_name_snapshot: Option<String>,
    pub batch: Option<String>,
    pub hsn_code: Option<String>,
    pub gst_rate_bps: i64,
    pub quantity_milli: i64,
    pub price_paise: i64,
    pub line_total_paise: i64,
}

#[derive(Debug, Serialize)]
pub struct PurchaseView {
    pub purchase_id: String,
    pub supplier_id: Option<String>,
    pub invoice_no: Option<String>,
    pub invoice_date: Option<String>,
    pub status: String, // draft, approved, void
    pub source: String, // manual, ocr
    pub subtotal_paise: i64,
    pub tax_total_paise: i64,
    pub total_paise: i64,
    pub created_by: String,
    pub created_at: String,
    pub items: Vec<PurchaseItemView>,
    pub image_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePurchaseRequest {
    pub source: String, // "manual" or "ocr"
}

#[derive(Debug, Deserialize)]
pub struct ListPurchasesQuery {
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePurchaseItemReq {
    pub purchase_item_id: Option<String>, // if missing, it's new
    pub product_id: Option<String>,
    pub product_name_snapshot: Option<String>,
    pub batch: Option<String>,
    pub hsn_code: Option<String>,
    pub gst_rate_bps: i64,
    pub quantity_milli: i64,
    pub price_paise: i64,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePurchaseRequest {
    pub supplier_id: Option<String>,
    pub invoice_no: Option<String>,
    pub invoice_date: Option<String>,
    pub items: Vec<UpdatePurchaseItemReq>,
}

async fn list_purchases(
    State(state): State<AppState>,
    user: AuthUser,
    Query(query): Query<ListPurchasesQuery>,
) -> Result<Json<Vec<PurchaseView>>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    // Simple list without items to save bandwidth
    let mut sql = "SELECT purchase_id, supplier_id, invoice_no, invoice_date, status, source, subtotal_paise, tax_total_paise, total_paise, created_by, created_at FROM purchases WHERE business_id = ?1".to_string();
    let mut params: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::from(user.business_id.clone())];

    if let Some(st) = &query.status {
        if !st.trim().is_empty() {
            sql.push_str(" AND status = ?");
            params.push(rusqlite::types::Value::from(st.trim().to_lowercase()));
        }
    }

    sql.push_str(" ORDER BY created_at DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
        Ok(PurchaseView {
            purchase_id: r.get(0)?,
            supplier_id: r.get(1)?,
            invoice_no: r.get(2)?,
            invoice_date: r.get(3)?,
            status: r.get(4)?,
            source: r.get(5)?,
            subtotal_paise: r.get(6)?,
            tax_total_paise: r.get(7)?,
            total_paise: r.get(8)?,
            created_by: r.get(9)?,
            created_at: r.get(10)?,
            items: vec![],
            image_id: None,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }

    Ok(Json(out))
}

async fn get_purchase(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<PurchaseView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    let image_id: Option<String> = conn
        .query_row(
            "SELECT image_id FROM bill_images WHERE purchase_id = ?1 ORDER BY created_at DESC LIMIT 1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .ok();

    let mut view = conn.query_row(
        "SELECT purchase_id, supplier_id, invoice_no, invoice_date, status, source, subtotal_paise, tax_total_paise, total_paise, created_by, created_at FROM purchases WHERE business_id = ?1 AND purchase_id = ?2",
        rusqlite::params![user.business_id, id],
        |r| {
            Ok(PurchaseView {
                purchase_id: r.get(0)?,
                supplier_id: r.get(1)?,
                invoice_no: r.get(2)?,
                invoice_date: r.get(3)?,
                status: r.get(4)?,
                source: r.get(5)?,
                subtotal_paise: r.get(6)?,
                tax_total_paise: r.get(7)?,
                total_paise: r.get(8)?,
                created_by: r.get(9)?,
                created_at: r.get(10)?,
                items: vec![],
                image_id,
            })
        }
    ).map_err(|_| ApiError::NotFound("Purchase not found".into()))?;

    let mut stmt = conn.prepare("SELECT purchase_item_id, product_id, product_name_snapshot, batch, hsn_code, gst_rate_bps, quantity_milli, price_paise, line_total_paise FROM purchase_items WHERE purchase_id = ?1")?;
    let items_iter = stmt.query_map(rusqlite::params![id], |r| {
        Ok(PurchaseItemView {
            purchase_item_id: r.get(0)?,
            product_id: r.get(1)?,
            product_name_snapshot: r.get(2)?,
            batch: r.get(3)?,
            hsn_code: r.get(4)?,
            gst_rate_bps: r.get(5)?,
            quantity_milli: r.get(6)?,
            price_paise: r.get(7)?,
            line_total_paise: r.get(8)?,
        })
    })?;

    for i in items_iter {
        view.items.push(i?);
    }

    Ok(Json(view))
}

async fn create_purchase(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreatePurchaseRequest>,
) -> Result<Json<PurchaseView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let purchase_id = new_id();
    let now = now_iso();

    tx.execute(
        "INSERT INTO purchases (purchase_id, business_id, status, source, subtotal_paise, tax_total_paise, total_paise, created_by, created_at, updated_at) VALUES (?1, ?2, 'draft', ?3, 0, 0, 0, ?4, ?5, ?5)",
        rusqlite::params![purchase_id, user.business_id, req.source, user.user_id, now],
    )?;

    tx.commit()?;

    Ok(Json(PurchaseView {
        purchase_id,
        supplier_id: None,
        invoice_no: None,
        invoice_date: None,
        status: "draft".to_string(),
        source: req.source,
        subtotal_paise: 0,
        tax_total_paise: 0,
        total_paise: 0,
        created_by: user.user_id,
        created_at: now,
        items: vec![],
        image_id: None,
    }))
}

async fn update_purchase(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdatePurchaseRequest>,
) -> Result<Json<PurchaseView>, ApiError> {
    {
        let mut conn = state.db.lock().expect("db mutex poisoned");
        let tx = conn.transaction()?;

        let status: String = tx
            .query_row(
                "SELECT status FROM purchases WHERE purchase_id = ?1 AND business_id = ?2",
                rusqlite::params![id, user.business_id],
                |r| r.get(0),
            )
            .map_err(|_| ApiError::NotFound("Purchase not found".into()))?;

        if status != "draft" {
            return Err(ApiError::Forbidden(
                "Only draft purchases can be edited".into(),
            ));
        }

        // Process items & recalculate totals server-side
        tx.execute(
            "DELETE FROM purchase_items WHERE purchase_id = ?1",
            rusqlite::params![id],
        )?;

        let mut subtotal: i64 = 0;
        let mut tax_total: i64 = 0;

        for item in req.items {
            if item.quantity_milli <= 0 {
                return Err(ApiError::BadRequest("Quantity must be positive".into()));
            }

            let item_id = item.purchase_item_id.unwrap_or_else(new_id);
            let line_total =
                (item.quantity_milli as f64 / 1000.0 * item.price_paise as f64).round() as i64;
            let line_tax = (line_total as f64 * item.gst_rate_bps as f64 / 10000.0).round() as i64;

            subtotal += line_total;
            tax_total += line_tax;

            tx.execute(
                "INSERT INTO purchase_items (purchase_item_id, purchase_id, product_id, product_name_snapshot, batch, hsn_code, gst_rate_bps, quantity_milli, price_paise, line_total_paise) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![item_id, id, item.product_id, item.product_name_snapshot, item.batch, item.hsn_code, item.gst_rate_bps, item.quantity_milli, item.price_paise, line_total]
            )?;
        }

        let grand_total = subtotal + tax_total;
        let now = now_iso();

        tx.execute(
            "UPDATE purchases SET supplier_id = ?1, invoice_no = ?2, invoice_date = ?3, subtotal_paise = ?4, tax_total_paise = ?5, total_paise = ?6, updated_at = ?7 WHERE purchase_id = ?8",
            rusqlite::params![req.supplier_id, req.invoice_no, req.invoice_date, subtotal, tax_total, grand_total, now, id]
        )?;

        tx.commit()?;
    }

    // Redirect to get_purchase to return full nested object safely without duplicating logic
    get_purchase(State(state), user, Path(id)).await
}

async fn approve_purchase(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let status: String = tx
        .query_row(
            "SELECT status FROM purchases WHERE purchase_id = ?1 AND business_id = ?2",
            rusqlite::params![id, user.business_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("Purchase not found".into()))?;

    if status != "draft" {
        return Err(ApiError::Conflict("Purchase is already processed".into()));
    }

    // Verify all items are resolved
    let unresolved: i64 = tx.query_row(
        "SELECT COUNT(*) FROM purchase_items WHERE purchase_id = ?1 AND product_id IS NULL",
        rusqlite::params![id],
        |r| r.get(0),
    )?;

    if unresolved > 0 {
        return Err(ApiError::BadRequest(
            "All products must be matched before approval".into(),
        ));
    }

    // Integrate with Inventory
    let mut stmt =
        tx.prepare("SELECT product_id, quantity_milli FROM purchase_items WHERE purchase_id = ?1")?;
    let items = stmt
        .query_map(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let now = now_iso();

    for (prod_id, qty) in items {
        // 1. Get current stock
        let current_stock: i64 = tx.query_row(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![user.business_id, prod_id],
            |r| r.get(0)
        )?;

        // 2. Insert transaction
        let tx_id = new_id();
        tx.execute(
            "INSERT INTO inventory_transactions (transaction_id, business_id, product_id, transaction_type, quantity_milli, reference_id, reason, created_by, created_at)
             VALUES (?1, ?2, ?3, 'purchase', ?4, ?5, NULL, ?6, ?7)",
             rusqlite::params![tx_id, user.business_id, prod_id, qty, id, user.user_id, now]
        )?;

        // 3. Update cache
        tx.execute(
            "UPDATE products SET stock_qty_milli = ?1, updated_at = ?2 WHERE product_id = ?3 AND business_id = ?4",
            rusqlite::params![current_stock + qty, now, prod_id, user.business_id]
        )?;
    }

    tx.execute(
        "UPDATE purchases SET status = 'approved', approved_at = ?1, updated_at = ?1 WHERE purchase_id = ?2",
        rusqlite::params![now, id]
    )?;

    crate::audit::record(
        &tx,
        &user.business_id,
        Some(&user.user_id),
        "purchase_approved",
        "purchase",
        Some(&id),
        "{}",
    )?;

    tx.commit()?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

async fn cancel_purchase(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let status: String = tx
        .query_row(
            "SELECT status FROM purchases WHERE purchase_id = ?1 AND business_id = ?2",
            rusqlite::params![id, user.business_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("Purchase not found".into()))?;

    if status != "draft" {
        return Err(ApiError::Conflict(
            "Only draft purchases can be cancelled directly".into(),
        ));
    }

    tx.execute(
        "UPDATE purchases SET status = 'void', updated_at = ?1 WHERE purchase_id = ?2",
        rusqlite::params![now_iso(), id],
    )?;

    tx.commit()?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}

fn validate_raster_image(bytes: &[u8]) -> Result<(&'static str, &'static str), ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::BadRequest("Uploaded image is empty".into()));
    }
    if bytes.len() > MAX_BILL_IMAGE_BYTES {
        return Err(ApiError::PayloadTooLarge(
            "File exceeds 10 MiB limit".into(),
        ));
    }

    const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() >= 8 && bytes[0..8] == PNG_SIG {
        return Ok(("image/png", "png"));
    }

    if bytes.len() >= 3 && bytes[0..3] == [0xFF, 0xD8, 0xFF] {
        return Ok(("image/jpeg", "jpg"));
    }

    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok(("image/webp", "webp"));
    }

    Err(ApiError::BadRequest(
        "Unsupported image format. Only PNG, JPEG, and WebP raster images are allowed".into(),
    ))
}

async fn upload_bill_image(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Validate purchase
    {
        let conn = state.db.lock().expect("db mutex poisoned");
        let status: String = conn
            .query_row(
                "SELECT status FROM purchases WHERE purchase_id = ?1 AND business_id = ?2",
                rusqlite::params![id, user.business_id],
                |r| r.get(0),
            )
            .map_err(|_| ApiError::NotFound("Purchase not found".into()))?;
        if status != "draft" {
            return Err(ApiError::Forbidden(
                "Cannot attach images to non-draft purchase".into(),
            ));
        }
    }

    let mut image_bytes = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::PayloadTooLarge("File exceeds 10 MiB limit".into())
        } else {
            ApiError::BadRequest(format!("Failed to parse multipart payload: {e}"))
        }
    })? {
        if field.name() == Some("file") {
            let bytes = field.bytes().await.map_err(|e| {
                if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::PayloadTooLarge("File exceeds 10 MiB limit".into())
                } else {
                    ApiError::BadRequest(format!("Failed to read file: {e}"))
                }
            })?;
            image_bytes = Some(bytes);
        }
    }

    let bytes = image_bytes.ok_or_else(|| ApiError::BadRequest("No file provided".into()))?;
    let (mime, ext) = validate_raster_image(&bytes)?;
    let sha256_hex = hex::encode(sha2::Sha256::digest(&bytes));

    if !state.upload_dir.exists() {
        fs::create_dir_all(&state.upload_dir)
            .await
            .map_err(|e| ApiError::Internal(format!("failed to create upload dir: {e}")))?;
    }

    let file_path = state.upload_dir.join(format!("{sha256_hex}.{ext}"));
    fs::write(&file_path, &bytes)
        .await
        .map_err(|e| ApiError::Internal(format!("failed to save upload: {e}")))?;

    let image_id = new_id();
    let now = now_iso();

    let conn = state.db.lock().expect("db mutex poisoned");
    conn.execute(
        "INSERT INTO bill_images (image_id, purchase_id, path, sha256_hex, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![image_id, id, file_path.to_str().unwrap(), sha256_hex, now],
    )?;

    Ok(Json(serde_json::json!({
        "image_id": image_id,
        "sha256": sha256_hex,
        "size_bytes": bytes.len(),
        "mime": mime,
    })))
}

async fn get_bill_image(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let path: String = {
        let conn = state.db.lock().expect("db mutex poisoned");
        conn.query_row(
            "SELECT b.path FROM bill_images b JOIN purchases p ON b.purchase_id = p.purchase_id WHERE p.purchase_id = ?1 AND p.business_id = ?2 ORDER BY b.created_at DESC LIMIT 1",
            rusqlite::params![id, user.business_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("Bill image not found".into()))?
    };

    let bytes = fs::read(&path)
        .await
        .map_err(|_| ApiError::NotFound("Bill image file not found on disk".into()))?;

    let mime = if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".webp") {
        "image/webp"
    } else {
        "image/jpeg"
    };

    Ok(([(axum::http::header::CONTENT_TYPE, mime)], bytes).into_response())
}

use sha2::Digest;

#[derive(Debug, Deserialize)]
pub struct OcrRequest {
    pub image_id: String,
}

async fn process_ocr(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<OcrRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // 1. Validate purchase is draft and belongs to this business
    let image_path: String = {
        let conn = state.db.lock().expect("db mutex poisoned");
        let status: String = conn
            .query_row(
                "SELECT status FROM purchases WHERE purchase_id = ?1 AND business_id = ?2",
                rusqlite::params![id, user.business_id],
                |r| r.get(0),
            )
            .map_err(|_| ApiError::NotFound("Purchase not found".into()))?;

        if status != "draft" {
            return Err(ApiError::Forbidden("Cannot run OCR on non-draft".into()));
        }

        conn.query_row(
            "SELECT path FROM bill_images WHERE image_id = ?1 AND purchase_id = ?2",
            rusqlite::params![req.image_id, id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("Image not found on this purchase".into()))?
    };

    // 2. Run OCR & Deterministic Bill Parser
    let (payload, parsed) = crate::ocr::execute_ocr(
        StdPath::new(&image_path),
        state.paddle_ocr_cmd.as_deref(),
        state.environment,
        std::time::Duration::from_secs(state.ocr_timeout_secs),
    )
    .await?;

    let ocr_id = new_id();
    let text_json = serde_json::to_string(&payload.tokens).unwrap_or_else(|_| "[]".to_string());
    let boxes_json = "[]".to_string();
    let now = now_iso();

    let conn = state.db.lock().expect("db mutex poisoned");
    conn.execute(
        "INSERT INTO ocr_results (ocr_id, image_id, text_json, boxes_json, confidence, engine_version, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![ocr_id, req.image_id, text_json, boxes_json, payload.confidence, payload.engine_version, now],
    )?;

    Ok(Json(serde_json::json!({
        "status": "completed",
        "ocr_id": ocr_id,
        "parsed": parsed,
    })))
}
