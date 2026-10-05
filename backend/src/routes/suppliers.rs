use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/suppliers", get(list_suppliers))
        .route("/api/suppliers", post(create_supplier))
}

#[derive(Debug, Serialize)]
pub struct SupplierView {
    pub supplier_id: String,
    pub name: String,
    pub contact: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSupplierRequest {
    pub name: String,
    pub contact: Option<String>,
}

async fn list_suppliers(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<SupplierView>>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    let mut stmt = conn.prepare("SELECT supplier_id, name, contact, created_at FROM suppliers WHERE business_id = ?1 ORDER BY name ASC")?;
    let rows = stmt.query_map(rusqlite::params![user.business_id], |r| {
        Ok(SupplierView {
            supplier_id: r.get(0)?,
            name: r.get(1)?,
            contact: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    
    Ok(Json(out))
}

async fn create_supplier(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateSupplierRequest>,
) -> Result<Json<SupplierView>, ApiError> {
    let mut conn = state.db.lock().expect("db mutex poisoned");
    let tx = conn.transaction()?;

    let supplier_id = new_id();
    let now = now_iso();

    tx.execute(
        "INSERT INTO suppliers (supplier_id, business_id, name, contact, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![supplier_id, user.business_id, req.name.trim(), req.contact, now, now],
    )?;

    tx.commit()?;

    Ok(Json(SupplierView {
        supplier_id,
        name: req.name.trim().to_string(),
        contact: req.contact,
        created_at: now,
    }))
}
