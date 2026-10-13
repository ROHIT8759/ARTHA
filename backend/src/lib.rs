//! ARTHA server library crate.
//!
//! Split from `main.rs` so integration tests (and `main.rs` itself) can build
//! the same Axum `Router` without spinning up a real TCP listener.

pub mod audit;
pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod ids;
pub mod logging;
pub mod models;
pub mod ocr;
pub mod routes;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use rusqlite::Connection;

/// Shared application state handed to every route handler.
///
/// V0 uses a single `rusqlite::Connection` behind a mutex rather than a pool:
/// SQLite serializes writers internally anyway, a shop-PC's request volume is
/// low, and this keeps the concurrency story easy to reason about. Revisit
/// (e.g. `r2d2_sqlite`) only if load testing in V1 shows contention.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub backup_dir: PathBuf,
    pub backup_retention: usize,
    pub upload_dir: PathBuf,
    pub environment: crate::config::Environment,
    pub paddle_ocr_cmd: Option<String>,
    pub ocr_timeout_secs: u64,
}

impl AppState {
    /// Convenience constructor for tests that don't exercise backup/restore
    /// directly: a harmless scratch backup dir (unique per process, so
    /// parallel `cargo test` runs never collide) and the documented default
    /// retention. Deliberately not `#[cfg(test)]`-gated — like
    /// `db::open_in_memory`, integration tests under `tests/` link this
    /// crate as a normal (non-`cfg(test)`) dependency, so that attribute
    /// would make this disappear for them, not just for release builds.
    pub fn for_tests(conn: Connection) -> AppState {
        AppState {
            db: Arc::new(Mutex::new(conn)),
            backup_dir: std::env::temp_dir()
                .join(format!("artha_test_backups_{}", std::process::id())),
            backup_retention: 24,
            upload_dir: std::env::temp_dir()
                .join(format!("artha_test_uploads_{}", std::process::id())),
            environment: crate::config::Environment::Development,
            paddle_ocr_cmd: None,
            ocr_timeout_secs: 30,
        }
    }
}

/// Build the full router. Used by both `main.rs` and tests.
pub fn build_app(state: AppState) -> Router {
    routes::router(state)
}
