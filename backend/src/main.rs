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
    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        backup_dir: config.backup_dir.clone(),
        backup_retention: config.backup_retention,
        upload_dir: config.upload_dir.clone(),
        environment: config.environment,
        paddle_ocr_cmd: config.paddle_ocr_cmd.clone(),
        ocr_timeout_secs: config.ocr_timeout_secs,
    };
    spawn_backup_scheduler(state.clone(), config.backup_interval_secs);
    let app = build_app(state);

    let addr = config.socket_addr();
    tracing::info!(db = %config.db_path.display(), %addr, env = ?config.environment, backup_dir = %config.backup_dir.display(), backup_interval_secs = config.backup_interval_secs, ocr_cmd = ?config.paddle_ocr_cmd, ocr_timeout_secs = config.ocr_timeout_secs, "starting artha-server");
    println!("ARTHA server listening on http://{addr}");
    println!("Staff on this Wi-Fi/LAN can connect using this PC's local IP instead of {}, e.g. http://<this-pc-ip>:{}", config.host, addr.port());

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind address");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl_c");
    tracing::info!("shutdown signal received");
}

/// In-process scheduled backup loop, per spec §9's "hourly" default.
///
/// Deliberately simple for this phase: a Tokio task tied to the server's own
/// lifetime, not an OS-level scheduler or separate process. This means
/// scheduled backups only happen while the backend is running — if the
/// owner's PC is off or the process isn't started, no background backup
/// fires. That's an accepted, documented limitation for V0.3; an OS-level
/// scheduler is an installer/packaging concern (spec build-order #14), out
/// of scope here. A failed backup attempt is logged and the loop continues
/// rather than panicking the whole task, so one bad backup cycle doesn't
/// silently stop all future ones.
fn spawn_backup_scheduler(state: AppState, interval_secs: u64) {
    if interval_secs == 0 {
        tracing::warn!("ARTHA_BACKUP_INTERVAL_SECS=0: scheduled backups disabled");
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        // First tick fires immediately; skip it so startup doesn't always
        // take a backup of a database that may be empty from `setup`.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match artha_server::routes::backup::create_backup(&state) {
                Ok(view) => {
                    tracing::info!(backup_id = %view.backup_id, size_bytes = view.size_bytes, "scheduled backup completed")
                }
                Err(e) => tracing::error!(error = ?e, "scheduled backup failed"),
            }
        }
    });
}
