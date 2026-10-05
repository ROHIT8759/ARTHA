//! ARTHA server entry point.
//!
//! Binds to a LAN address (default `0.0.0.0:8080`) so staff devices on the
//! shop Wi-Fi can reach it, not just `localhost` on the owner's PC. Per
//! spec §12/§11, this is *not* meant to be exposed to the public internet —
//! the owner is expected to be on a private/home-router LAN, and production
//! hardening (explicit firewall guidance, bind-address config UI) is a V1
//! gate, not a V0 concern.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use artha_server::{build_app, db, AppState};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let db_path = std::env::var("ARTHA_DB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/artha.db"));

    let bind_addr = std::env::var("ARTHA_BIND").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    let addr: SocketAddr = bind_addr.parse().expect("ARTHA_BIND must be a valid host:port");

    let conn = db::open_and_migrate(&db_path).expect("failed to open/migrate database");
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);

    tracing::info!(db = %db_path.display(), %addr, "starting artha-server");
    println!("ARTHA server listening on http://{addr}");
    println!("Staff on this Wi-Fi/LAN can connect using this PC's local IP instead of 0.0.0.0, e.g. http://<this-pc-ip>:{}", addr.port());

    let listener = tokio::net::TcpListener::bind(addr).await.expect("failed to bind address");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.expect("failed to listen for ctrl_c");
    tracing::info!("shutdown signal received");
}
