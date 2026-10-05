//! Product catalog + QR identity (spec §6.2, §14).
//!
//! V0 scope: CRUD plus QR lookup. Any authenticated user (owner or staff)
//! may manage products — per-staff permission granularity is explicitly a
//! V1 feature ("Staff management and permissions"), so we don't invent a
//! half-finished version of it here.

use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::models::{CreateProductRequest, ProductView, UpdateProductRequest};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/products", get(list_products).post(create_product))
        .route(
            "/api/products/:product_id",
            get(get_product).patch(update_product),
        )
        .route("/api/products/by-qr/:qr_code", get(get_product_by_qr))
}

fn row_to_product(r: &rusqlite::Row) -> rusqlite::Result<ProductView> {
    Ok(ProductView {
        product_id: r.get(0)?,
        name: r.get(1)?,
        batch: r.get(2)?,
        hsn_code: r.get(3)?,
        gst_rate_bps: r.get(4)?,
        qr_code: r.get(5)?,
        price_paise: r.get(6)?,
        cost_paise: r.get(7)?,
        stock_qty_milli: r.get(8)?,
        unit: r.get(9)?,
        status: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

const SELECT_COLUMNS: &str = "product_id, name, batch, hsn_code, gst_rate_bps, qr_code, price_paise, cost_paise, stock_qty_milli, unit, status, created_at, updated_at";

async fn list_products(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<ProductView>>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let sql = format!(
        "SELECT {SELECT_COLUMNS} FROM products WHERE business_id = ?1 ORDER BY name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params![user.business_id], row_to_product)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(Json(out))
}

async fn create_product(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateProductRequest>,
) -> Result<Json<ProductView>, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("name is required".into()));
    }
    if req.qr_code.trim().is_empty() {
        return Err(ApiError::BadRequest("qr_code is required".into()));
    }
    if req.price_paise < 0 || req.cost_paise < 0 || req.gst_rate_bps < 0 {
        return Err(ApiError::BadRequest("price, cost and gst rate must be non-negative".into()));
    }

    let conn = state.db.lock().expect("db mutex poisoned");

    let dup: i64 = conn.query_row(
        "SELECT COUNT(*) FROM products WHERE business_id = ?1 AND qr_code = ?2",
        rusqlite::params![user.business_id, req.qr_code.trim()],
        |r| r.get(0),
    )?;
    if dup > 0 {
        return Err(ApiError::Conflict("a product with this QR code already exists".into()));
    }

    let product_id = new_id();
    let ts = now_iso();
    let unit = req.unit.unwrap_or_else(|| "pcs".to_string());

    conn.execute(
        "INSERT INTO products (product_id, business_id, name, batch, hsn_code, gst_rate_bps, qr_code, price_paise, cost_paise, stock_qty_milli, unit, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, ?10, 'active', ?11, ?11)",
        rusqlite::params![
            product_id, user.business_id, req.name.trim(), req.batch, req.hsn_code,
            req.gst_rate_bps, req.qr_code.trim(), req.price_paise, req.cost_paise, unit, ts,
        ],
    )?;

    crate::audit::record(&conn, &user.business_id, Some(&user.user_id), "create_product", "product", Some(&product_id), "{}")?;

    fetch_one(&conn, &user.business_id, &product_id)
}

async fn get_product(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<String>,
) -> Result<Json<ProductView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    fetch_one(&conn, &user.business_id, &product_id)
}

async fn get_product_by_qr(
    State(state): State<AppState>,
    user: AuthUser,
    Path(qr_code): Path<String>,
) -> Result<Json<ProductView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let sql = format!("SELECT {SELECT_COLUMNS} FROM products WHERE business_id = ?1 AND qr_code = ?2");
    conn.query_row(&sql, rusqlite::params![user.business_id, qr_code], row_to_product)
        .map(Json)
        .map_err(|_| ApiError::NotFound("no product matches this QR code".into()))
}

async fn update_product(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<String>,
    Json(req): Json<UpdateProductRequest>,
) -> Result<Json<ProductView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    // Fetch-then-write (rather than a dynamic SET clause) keeps this simple
    // and correct for V0's low write volume; a partial-update query builder
    // can come later if the handler list grows.
    let current = fetch_one(&conn, &user.business_id, &product_id)?.0;

    let name = req.name.unwrap_or(current.name);
    let batch = req.batch.or(current.batch);
    let hsn_code = req.hsn_code.or(current.hsn_code);
    let gst_rate_bps = req.gst_rate_bps.unwrap_or(current.gst_rate_bps);
    let price_paise = req.price_paise.unwrap_or(current.price_paise);
    let cost_paise = req.cost_paise.unwrap_or(current.cost_paise);
    let unit = req.unit.unwrap_or(current.unit);
    let status = req.status.unwrap_or(current.status);

    if status != "active" && status != "archived" {
        return Err(ApiError::BadRequest("status must be 'active' or 'archived'".into()));
    }

    conn.execute(
        "UPDATE products SET name = ?1, batch = ?2, hsn_code = ?3, gst_rate_bps = ?4, price_paise = ?5, cost_paise = ?6, unit = ?7, status = ?8, updated_at = ?9
         WHERE product_id = ?10 AND business_id = ?11",
        rusqlite::params![name, batch, hsn_code, gst_rate_bps, price_paise, cost_paise, unit, status, now_iso(), product_id, user.business_id],
    )?;

    crate::audit::record(&conn, &user.business_id, Some(&user.user_id), "update_product", "product", Some(&product_id), "{}")?;

    fetch_one(&conn, &user.business_id, &product_id)
}

fn fetch_one(conn: &rusqlite::Connection, business_id: &str, product_id: &str) -> Result<Json<ProductView>, ApiError> {
    let sql = format!("SELECT {SELECT_COLUMNS} FROM products WHERE business_id = ?1 AND product_id = ?2");
    conn.query_row(&sql, rusqlite::params![business_id, product_id], row_to_product)
        .map(Json)
        .map_err(|_| ApiError::NotFound("product not found".into()))
}
