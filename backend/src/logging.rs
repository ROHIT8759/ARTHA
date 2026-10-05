//! Logging setup. One function, called once at startup — this is
//! deliberately not a bigger "observability" module; add one if/when V1
//! actually needs structured log shipping, log files, etc.

use crate::config::Config;

/// Initialize the global `tracing` subscriber.
///
/// Level comes from `RUST_LOG` if set (the conventional escape hatch for
/// ad-hoc debugging, e.g. `RUST_LOG=artha_server=debug cargo run`),
/// otherwise from `config.log_level`. ANSI color codes are left on in
/// development (readable in a terminal) and off in production (so logs
/// redirected to a file or piped to a log collector don't carry escape
/// sequences).
pub fn init(config: &Config) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(config.log_level.clone()));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(config.environment.is_development())
        .init();
}
