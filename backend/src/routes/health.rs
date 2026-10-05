//! Liveness check. Deliberately has no auth and touches no DB state beyond a
//! trivial query, so it stays useful as a "is the process up at all" probe
//! even if something else is broken.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/api/health", get(health))
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let ok = state.db.lock().map(|c| c.is_autocommit()).unwrap_or(false);
    Json(json!({
        "status": if ok { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
