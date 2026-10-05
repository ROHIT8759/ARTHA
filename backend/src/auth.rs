//! Password hashing, session tokens, and the Axum extractor that enforces
//! authentication + role-based authorization server-side.
//!
//! Per spec §11: "Role-based authorization enforced by the Rust backend,
//! not only the UI." Every handler that needs a logged-in user takes
//! `AuthUser` as an extractor argument; handlers that need owner-only access
//! call `AuthUser::require_owner`. There is no separate "trust the frontend"
//! path — a direct API call with a staff token hits the same check.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::error::ApiError;
use crate::ids::{new_id, now_iso};
use crate::models::Role;
use crate::AppState;

/// Sessions are opaque random tokens handed to the client; only the SHA-256
/// hash is stored server-side, so a leaked database backup doesn't hand out
/// live bearer tokens.
const SESSION_TTL_HOURS: i64 = 12;

pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::Internal(format!("hash error: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create a session row for `user_id` and return the plaintext token (shown
/// to the client exactly once) plus its expiry.
pub fn create_session(conn: &Connection, user_id: &str) -> Result<(String, String), ApiError> {
    let token = random_token();
    let token_hash = hash_token(&token);
    let created_at = now_iso();
    let expires_at = add_hours_iso(&created_at, SESSION_TTL_HOURS);

    conn.execute(
        "INSERT INTO sessions (session_id, user_id, token_hash, created_at, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![new_id(), user_id, token_hash, created_at, expires_at],
    )?;

    Ok((token, expires_at))
}

/// Naive "add N hours to an ISO timestamp" that avoids pulling in a date
/// crate. Only needs to be monotonic and roughly correct for session expiry.
fn add_hours_iso(iso: &str, hours: i64) -> String {
    // Parse back to a unix-ish second count is more code than it's worth for
    // V0; instead store expiry as (created epoch millis + ttl) and format.
    // Simpler: recompute from SystemTime "now" at call time since this is
    // always called immediately after `now_iso()`.
    use std::time::{SystemTime, UNIX_EPOCH};
    let _ = iso; // created_at already captured by caller; kept for clarity/logging
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let future = now + std::time::Duration::from_secs((hours * 3600) as u64);
    crate::ids::format_unix_public(future.as_secs(), future.subsec_millis())
}

pub struct AuthUser {
    pub user_id: String,
    pub business_id: String,
    pub username: String,
    pub role: Role,
}

impl AuthUser {
    pub fn require_owner(&self) -> Result<(), ApiError> {
        if self.role != Role::Owner {
            return Err(ApiError::Forbidden("owner access required".into()));
        }
        Ok(())
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| ApiError::Unauthorized("missing Authorization header".into()))?;

        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| ApiError::Unauthorized("expected Bearer token".into()))?;
        let token_hash = hash_token(token);

        let conn = state.db.lock().expect("db mutex poisoned");

        let row = conn
            .query_row(
                "SELECT s.user_id, s.expires_at, s.revoked_at, u.business_id, u.username, u.role, u.status
                 FROM sessions s JOIN users u ON u.user_id = s.user_id
                 WHERE s.token_hash = ?1",
                rusqlite::params![token_hash],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .map_err(|_| ApiError::Unauthorized("invalid or expired session".into()))?;

        let (user_id, expires_at, revoked_at, business_id, username, role_str, status) = row;

        if revoked_at.is_some() {
            return Err(ApiError::Unauthorized("session revoked".into()));
        }
        if status != "active" {
            return Err(ApiError::Forbidden("account disabled".into()));
        }
        if expires_at.as_str() < now_iso().as_str() {
            return Err(ApiError::Unauthorized("session expired".into()));
        }

        let role = Role::from_str(&role_str)
            .ok_or_else(|| ApiError::Internal(format!("unknown role in db: {role_str}")))?;

        Ok(AuthUser { user_id, business_id, username, role })
    }
}
