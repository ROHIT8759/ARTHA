//! ARTHA server entry point.
//!
//! Binds to a LAN address (default `0.0.0.0:8080`) so staff devices on the
//! shop Wi-Fi can reach it, not just `localhost` on the owner's PC. Per
//! spec §12/§11, this is *not* meant to be exposed to the public internet —
//! the owner is expected to be on a private/home-router LAN, and production
//! hardening (explicit firewall guidance, bind-address config UI) is a V1
//! gate, not a V0 concern.

use std::sync::{Arc, Mutex};

use artha_server::{build_app, config::Config, db, logging, AppState};

#[tokio::main]
async fn main() {
    let config = Config::from_env().unwrap_or_else(|e| {
        // Logging isn't initialized yet (it needs the config we just failed
        // to load), so this one error goes straight to stderr.
        eprintln!("invalid configuration: {e}");
        std::process::exit(1);
    });

    logging::init(&config);

    let conn = db::open_and_migrate(&config.db_path).expect("failed to open/migrate database");
    let state = AppState { db: Arc::new(Mutex::new(conn)) };
    let app = build_app(state);

    let addr = config.socket_addr();
    tracing::info!(db = %config.db_path.display(), %addr, env = ?config.environment, "starting artha-server");
    println!("ARTHA server listening on http://{addr}");
    println!("Staff on this Wi-Fi/LAN can connect using this PC's local IP instead of {}, e.g. http://<this-pc-ip>:{}", config.host, addr.port());

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
