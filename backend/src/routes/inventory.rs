//! Inventory Ledger Engine (Phase 05).
//!
//! Authoritative source of stock balances. Every change must be recorded
//! as an immutable transaction. The `stock_qty_milli` on the `products`
//! table is strictly a cache for fast list rendering.

use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/inventory/:product_id", get(get_inventory_status))
        .route("/api/inventory/:product_id/history", get(get_inventory_history))
        .route("/api/inventory/opening", post(post_opening_stock))
        .route("/api/inventory/adjust", post(post_adjustment))
}

#[derive(Debug, Serialize)]
pub struct InventoryStatusView {
    pub product_id: String,
    pub current_stock_milli: i64,
    pub status: String, // from products table (active/archived)
}

#[derive(Debug, Serialize)]
pub struct InventoryTransactionView {
    pub transaction_id: String,
    pub product_id: String,
    pub transaction_type: String,
    pub quantity_milli: i64,
    pub reference_id: Option<String>,
    pub reason: Option<String>,
    pub created_by: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct OpeningStockRequest {
    pub product_id: String,
    pub quantity_milli: i64,
}

#[derive(Debug, Deserialize)]
pub struct AdjustmentRequest {
    pub product_id: String,
    pub quantity_milli: i64,
    pub reason: String,
}

/// Retrieve the current authoritative stock and product status.
async fn get_inventory_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<String>,
) -> Result<Json<InventoryStatusView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    // We fetch the status to ensure the product exists and belongs to the business
    let status: String = conn
        .query_row(
            "SELECT status FROM products WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![user.business_id, product_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("product not found".into()))?;

    // The authoritative ledger balance
    let stock: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![user.business_id, product_id],
            |r| r.get(0),
        )?;

    Ok(Json(InventoryStatusView {
        product_id,
        current_stock_milli: stock,
        status,
    }))
}

/// Fetch ledger history for a specific product.
async fn get_inventory_history(
    State(state): State<AppState>,
    user: AuthUser,
    Path(product_id): Path<String>,
) -> Result<Json<Vec<InventoryTransactionView>>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    
    // Ensure product exists (and check access)
    let _ = conn.query_row(
        "SELECT 1 FROM products WHERE business_id = ?1 AND product_id = ?2",
        rusqlite::params![user.business_id, product_id],
        |_| Ok(()),
    ).map_err(|_| ApiError::NotFound("product not found".into()))?;

    let mut stmt = conn.prepare(
        "SELECT transaction_id, transaction_type, quantity_milli, reference_id, reason, created_by, created_at 
         FROM inventory_transactions 
         WHERE business_id = ?1 AND product_id = ?2 
         ORDER BY created_at DESC"
    )?;

    let rows = stmt.query_map(rusqlite::params![user.business_id, product_id], |r| {
        Ok(InventoryTransactionView {
            transaction_id: r.get(0)?,
            product_id: product_id.clone(),
            transaction_type: r.get(1)?,
            quantity_milli: r.get(2)?,
            reference_id: r.get(3)?,
            reason: r.get(4)?,
            created_by: r.get(5)?,
            created_at: r.get(6)?,
        })
    })?;

    let mut history = Vec::new();
    for row in rows {
        history.push(row?);
    }
    
    Ok(Json(history))
}

/// Helper function to encapsulate the atomic transaction insertion and constraint checking.
fn record_inventory_movement(
    conn: &mut rusqlite::Connection,
    business_id: &str,
    product_id: &str,
    user_id: &str,
    tx_type: &str,
    qty: i64,
    reason: Option<&str>,
    reference_id: Option<&str>,
) -> Result<(), ApiError> {
    if qty == 0 {
        return Err(ApiError::BadRequest("quantity must not be zero".into()));
    }

    let tx = conn.transaction()?;

    // 1. Verify product exists, belongs to business, and is active
    let status: String = tx
        .query_row(
            "SELECT status FROM products WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![business_id, product_id],
            |r| r.get(0),
        )
        .map_err(|_| ApiError::NotFound("product not found".into()))?;

    if status != "active" {
        return Err(ApiError::Forbidden("cannot change inventory for archived products".into()));
    }

    // 2. Prevent duplicate opening stock
    if tx_type == "opening" {
        let existing_opening: i64 = tx.query_row(
            "SELECT COUNT(*) FROM inventory_transactions WHERE business_id = ?1 AND product_id = ?2 AND transaction_type = 'opening'",
            rusqlite::params![business_id, product_id],
            |r| r.get(0),
        )?;
        if existing_opening > 0 {
            return Err(ApiError::Conflict("opening stock is already set for this product".into()));
        }
        if qty < 0 {
            return Err(ApiError::BadRequest("opening stock cannot be negative".into()));
        }
    }

    // 3. Calculate authoritative current stock and verify negative constraints
    let current_stock: i64 = tx
        .query_row(
            "SELECT COALESCE(SUM(quantity_milli), 0) FROM inventory_transactions WHERE business_id = ?1 AND product_id = ?2",
            rusqlite::params![business_id, product_id],
            |r| r.get(0),
        )?;

    let new_stock = current_stock + qty;
    if new_stock < 0 {
        return Err(ApiError::BadRequest("insufficient stock".into()));
    }

    let tx_id = new_id();
    let ts = now_iso();

    // 4. Insert authoritative transaction
    tx.execute(
        "INSERT INTO inventory_transactions (transaction_id, business_id, product_id, transaction_type, quantity_milli, reference_id, reason, created_by, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![tx_id, business_id, product_id, tx_type, qty, reference_id, reason, user_id, ts],
    )?;

    // 5. Update cached balance on products table
    tx.execute(
        "UPDATE products SET stock_qty_milli = ?1, updated_at = ?2 WHERE business_id = ?3 AND product_id = ?4",
        rusqlite::params![new_stock, ts, business_id, product_id],
    )?;

    // 6. Audit log for sensitive operations (opening, adjustment)
    if tx_type == "opening" || tx_type == "adjustment" {
        crate::audit::record(&tx, business_id, Some(user_id), &format!("inventory_{}", tx_type), "inventory_transaction", Some(&tx_id), &serde_json::json!({ "product_id": product_id, "quantity_milli": qty, "reason": reason }).to_string())?;
    }

    tx.commit()?;
    Ok(())
}

async fn post_opening_stock(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<OpeningStockRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    
    // Authorization: Owner or staff. The core product system currently doesn't 
    // fragment permissions beyond AuthUser access. 
    // If needed we can enforce require_owner() here in the future.

    record_inventory_movement(
        &mut conn,
        &user.business_id,
        &req.product_id,
        &user.user_id,
        "opening",
        req.quantity_milli,
        None,
        None,
    )?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

async fn post_adjustment(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<AdjustmentRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if req.reason.trim().is_empty() {
        return Err(ApiError::BadRequest("reason is required for manual adjustments".into()));
    }

    let mut conn = state.db.lock().expect("db mutex poisoned");

    record_inventory_movement(
        &mut conn,
        &user.business_id,
        &req.product_id,
        &user.user_id,
        "adjustment",
        req.quantity_milli,
        Some(req.reason.trim()),
        None,
    )?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}
