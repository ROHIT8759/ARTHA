//! Foundation-layer integration tests: these exercise the public API the
//! way an external binary/tool would (not internal unit tests), covering
//! app startup, database initialization + migrations, the health endpoint,
//! and configuration loading.
//!
//! Business-feature tests (auth, products, ...) live in `tests/smoke.rs`;
//! this file is scoped to the infrastructure layer only.

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU32, Ordering};

use artha_server::config::{Config, Environment};
use artha_server::{build_app, db, AppState};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn temp_db_path(label: &str) -> std::path::PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("artha_foundation_test_{label}_{}_{n}.db", std::process::id()))
}

fn remove_db_and_sidecars(path: &std::path::Path) {
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
}

/// "Application startup": building the real router over a real
/// (migrated) database and sending it a request must not panic. We don't
/// additionally bind a real TCP socket here — `axum::serve` + a listener
/// is a thin, well-tested wrapper around this same `Router`, and testing
/// it would mean adding an HTTP client dependency for one assertion.
#[tokio::test]
async fn application_starts_and_serves_a_request() {
    let conn = db::open_in_memory();
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);

    let resp = app
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .expect("router should handle the request without panicking");

    assert_eq!(resp.status(), StatusCode::OK);
}

/// Database initialization + migration execution, against a real file on
/// disk (not in-memory), the way the server actually starts up.
#[tokio::test]
async fn database_initializes_and_runs_migrations_on_a_real_file() {
    let path = temp_db_path("init");
    remove_db_and_sidecars(&path); // in case a previous failed run left one

    let conn = db::open_and_migrate(&path).expect("database should open and migrate");

    let schema_version: i64 = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
        .expect("migrations should have recorded a schema version");
    assert_eq!(schema_version, 1, "migration 0001_init.sql should be applied");

    // A representative table from that migration must actually exist.
    let table_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'products'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 1);

    drop(conn);
    remove_db_and_sidecars(&path);
}

/// GET /health (mounted as /api/health) responds with something that
/// clearly indicates the local server is running, and needs no auth token.
#[tokio::test]
async fn health_endpoint_reports_the_server_is_running() {
    let conn = db::open_in_memory();
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);

    let resp = app
        .oneshot(Request::builder().uri("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

/// Configuration loading, exercised through the public `Config` API
/// exactly as `main.rs` uses it (see `src/config.rs` for the full unit
/// test suite covering individual fields and error cases).
#[test]
fn configuration_loads_with_defaults_and_overrides() {
    use std::collections::HashMap;

    let defaults = Config::from_lookup(|_: &str| None).expect("defaults alone should be valid");
    assert_eq!(defaults.port, 8080);
    assert_eq!(defaults.environment, Environment::Development);

    let mut vars = HashMap::new();
    vars.insert("ARTHA_PORT", "9999");
    vars.insert("ARTHA_ENV", "production");
    let overridden = Config::from_lookup(|k: &str| vars.get(k).map(|v| v.to_string())).expect("valid overrides should load");
    assert_eq!(overridden.port, 9999);
    assert_eq!(overridden.environment, Environment::Production);
    assert_eq!(overridden.socket_addr().port(), 9999);
}
