//! Owner-only staff account management.
//!
//! Every handler here takes `AuthUser` and calls `require_owner()` before
//! touching the database — enforced in Rust, not left to the frontend to
//! hide a button. See spec §11.

use axum::extract::{Path, State};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::auth::{hash_password, AuthUser};
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::models::{CreateStaffRequest, UserView};
use crate::{audit, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/users", get(list_users).post(create_staff))
        .route("/api/users/:user_id", patch(update_user_status))
        .route("/api/users/:user_id/disable", post(disable_user))
}

async fn list_users(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<UserView>>, ApiError> {
    user.require_owner()?;
    let conn = state.db.lock().expect("db mutex poisoned");

    let mut stmt = conn.prepare(
        "SELECT user_id, username, role, status, created_at FROM users WHERE business_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map(rusqlite::params![user.business_id], |r| {
        Ok(UserView {
            user_id: r.get(0)?,
            username: r.get(1)?,
            role: r.get(2)?,
            status: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;

    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(Json(out))
}

async fn create_staff(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateStaffRequest>,
) -> Result<Json<UserView>, ApiError> {
    user.require_owner()?;

    if req.username.trim().is_empty() {
        return Err(ApiError::BadRequest("username is required".into()));
    }
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest("password must be at least 8 characters".into()));
    }

    let conn = state.db.lock().expect("db mutex poisoned");

    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM users WHERE business_id = ?1 AND username = ?2",
        rusqlite::params![user.business_id, req.username.trim()],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Err(ApiError::Conflict("username already exists".into()));
    }

    let new_user_id = new_id();
    let ts = now_iso();
    let password_hash = hash_password(&req.password)?;

    conn.execute(
        "INSERT INTO users (user_id, business_id, username, password_hash, role, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'staff', 'active', ?5, ?5)",
        rusqlite::params![new_user_id, user.business_id, req.username.trim(), password_hash, ts],
    )?;

    audit::record(&conn, &user.business_id, Some(&user.user_id), "create_staff", "user", Some(&new_user_id), "{}")?;

    Ok(Json(UserView {
        user_id: new_user_id,
        username: req.username.trim().to_string(),
        role: "staff".to_string(),
        status: "active".to_string(),
        created_at: ts,
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateUserStatusRequest {
    status: String,
}

async fn update_user_status(
    State(state): State<AppState>,
    user: AuthUser,
    Path(target_user_id): Path<String>,
    Json(req): Json<UpdateUserStatusRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    user.require_owner()?;
    if req.status != "active" && req.status != "disabled" {
        return Err(ApiError::BadRequest("status must be 'active' or 'disabled'".into()));
    }
    if target_user_id == user.user_id && req.status == "disabled" {
        return Err(ApiError::BadRequest("cannot disable your own account".into()));
    }

    let conn = state.db.lock().expect("db mutex poisoned");
    let changed = conn.execute(
        "UPDATE users SET status = ?1, updated_at = ?2 WHERE user_id = ?3 AND business_id = ?4",
        rusqlite::params![req.status, now_iso(), target_user_id, user.business_id],
    )?;
    if changed == 0 {
        return Err(ApiError::NotFound("user not found".into()));
    }

    audit::record(
        &conn,
        &user.business_id,
        Some(&user.user_id),
        "update_user_status",
        "user",
        Some(&target_user_id),
        &serde_json::json!({ "status": req.status }).to_string(),
    )?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// Convenience alias for the common case (`PATCH` with a body is easy to
/// forget the body on from a shell/curl); same effect as
/// `PATCH /api/users/:id { "status": "disabled" }`.
async fn disable_user(
    state: State<AppState>,
    user: AuthUser,
    path: Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    update_user_status(state, user, path, Json(UpdateUserStatusRequest { status: "disabled".into() })).await
}
