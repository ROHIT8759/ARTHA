//! First-run business + owner account creation, and login/logout.
//!
//! `POST /api/setup` is intentionally only allowed once: the first run of
//! the app walks the owner through creating the business and their own
//! account; after that it 409s, so it can't be used to spray extra owner
//! accounts onto a live shop database.

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};

use crate::auth::{create_session, hash_password, verify_password, AuthUser};
use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::models::{LoginRequest, LoginResponse, Role, SetupBusinessRequest, UserView};
use crate::{audit, AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/setup", axum::routing::get(setup_status).post(setup_business))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/me", axum::routing::get(me))
}

/// Public (no auth) — lets the frontend know whether first-run setup still
/// needs to happen, so it can route a fresh browser straight to `/setup`
/// instead of showing a login form with no account to log into yet.
async fn setup_status(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let exists: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0))?;
    Ok(Json(serde_json::json!({ "exists": exists > 0 })))
}

async fn setup_business(
    State(state): State<AppState>,
    Json(req): Json<SetupBusinessRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if req.business_name.trim().is_empty() {
        return Err(ApiError::BadRequest("business_name is required".into()));
    }
    if req.owner_username.trim().is_empty() {
        return Err(ApiError::BadRequest("owner_username is required".into()));
    }
    if req.owner_password.len() < 8 {
        return Err(ApiError::BadRequest("owner_password must be at least 8 characters".into()));
    }

    let conn = state.db.lock().expect("db mutex poisoned");

    let existing: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0))?;
    if existing > 0 {
        return Err(ApiError::Conflict("a business is already set up on this server".into()));
    }

    let business_id = new_id();
    let user_id = new_id();
    let ts = now_iso();
    let password_hash = hash_password(&req.owner_password)?;

    conn.execute(
        "INSERT INTO businesses (business_id, name, settings_json, created_at, updated_at) VALUES (?1, ?2, '{}', ?3, ?3)",
        rusqlite::params![business_id, req.business_name.trim(), ts],
    )?;
    conn.execute(
        "INSERT INTO users (user_id, business_id, username, password_hash, role, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'owner', 'active', ?5, ?5)",
        rusqlite::params![user_id, business_id, req.owner_username.trim(), password_hash, ts],
    )?;

    audit::record(&conn, &business_id, Some(&user_id), "setup_business", "business", Some(&business_id), "{}")?;

    let (token, expires_at) = create_session(&conn, &user_id)?;

    Ok(Json(LoginResponse {
        token,
        user: UserView {
            user_id,
            username: req.owner_username.trim().to_string(),
            role: Role::Owner.as_str().to_string(),
            status: "active".to_string(),
            created_at: ts,
        },
        expires_at,
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");

    let row = conn
        .query_row(
            "SELECT user_id, password_hash, role, status, created_at FROM users WHERE username = ?1",
            rusqlite::params![req.username.trim()],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        )
        .map_err(|_| ApiError::Unauthorized("invalid username or password".into()))?;

    let (user_id, password_hash, role, status, created_at) = row;

    // Verify the password even if the account is disabled, so a disabled
    // account doesn't leak its status to someone guessing passwords. We
    // just report the same "invalid username or password" either way below.
    if !verify_password(&req.password, &password_hash) {
        return Err(ApiError::Unauthorized("invalid username or password".into()));
    }
    if status != "active" {
        return Err(ApiError::Forbidden("account disabled".into()));
    }

    let (token, expires_at) = create_session(&conn, &user_id)?;

    Ok(Json(LoginResponse {
        token,
        user: UserView {
            user_id,
            username: req.username.trim().to_string(),
            role,
            status,
            created_at,
        },
        expires_at,
    }))
}

/// Lets the frontend recover "who am I / what's my role" after a page
/// reload without re-sending credentials — it only has the bearer token in
/// localStorage, not the user object.
async fn me(State(state): State<AppState>, user: AuthUser) -> Result<Json<UserView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let row = conn.query_row(
        "SELECT status, created_at FROM users WHERE user_id = ?1",
        rusqlite::params![user.user_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )?;
    Ok(Json(UserView {
        user_id: user.user_id,
        username: user.username,
        role: user.role.as_str().to_string(),
        status: row.0,
        created_at: row.1,
    }))
}

async fn logout(State(state): State<AppState>, user: AuthUser) -> Result<Json<serde_json::Value>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    conn.execute(
        "UPDATE sessions SET revoked_at = ?1 WHERE user_id = ?2 AND revoked_at IS NULL",
        rusqlite::params![now_iso(), user.user_id],
    )?;
    Ok(Json(serde_json::json!({ "status": "ok" })))
}
