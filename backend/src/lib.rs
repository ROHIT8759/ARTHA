//! ARTHA server library crate.
//!
//! Split from `main.rs` so integration tests (and `main.rs` itself) can build
//! the same Axum `Router` without spinning up a real TCP listener.

pub mod audit;
pub mod auth;
pub mod db;
pub mod error;
pub mod ids;
pub mod models;
pub mod routes;

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
}

/// Build the full router. Used by both `main.rs` and tests.
pub fn build_app(state: AppState) -> Router {
    routes::router(state)
}
