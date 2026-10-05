//! Application configuration: host, port, database path, log level, and
//! dev/production mode. Loaded from environment variables, with no cloud
//! configuration of any kind — per spec, this app has no mandatory cloud
//! dependency, so there is nothing here for a remote endpoint, API key, etc.
//!
//! `Config::from_env` is a thin wrapper around `Config::from_lookup`, which
//! takes any `Fn(&str) -> Option<String>` as its variable source. Tests use
//! a `HashMap` through `from_lookup` instead of mutating real process
//! environment variables, so they stay deterministic and safe to run in
//! parallel (`std::env::set_var` is process-global and races across tests).

use std::fmt;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

impl Environment {
    pub fn is_development(&self) -> bool {
        matches!(self, Environment::Development)
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub db_path: PathBuf,
    pub log_level: String,
    pub environment: Environment,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    InvalidPort(String),
    InvalidEnvironment(String),
    InvalidHost(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::InvalidPort(v) => write!(f, "ARTHA_PORT must be a valid port number, got {v:?}"),
            ConfigError::InvalidEnvironment(v) => {
                write!(f, "ARTHA_ENV must be 'development' or 'production', got {v:?}")
            }
            ConfigError::InvalidHost(v) => write!(f, "ARTHA_HOST is not a resolvable host, got {v:?}"),
        }
    }
}

impl std::error::Error for ConfigError {}

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_DB_PATH: &str = "data/artha.db";
const DEFAULT_LOG_LEVEL: &str = "info";

impl Config {
    /// Load configuration from the real process environment.
    pub fn from_env() -> Result<Config, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Load configuration from any key→value source. This indirection is
    /// what lets tests supply a fixed `HashMap` instead of touching real
    /// environment variables.
    pub fn from_lookup<F>(get: F) -> Result<Config, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let host = get("ARTHA_HOST").unwrap_or_else(|| DEFAULT_HOST.to_string());

        let port = match get("ARTHA_PORT") {
            Some(raw) => raw.parse::<u16>().map_err(|_| ConfigError::InvalidPort(raw))?,
            None => DEFAULT_PORT,
        };

        let db_path = get("ARTHA_DB_PATH").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_DB_PATH));

        let log_level = get("ARTHA_LOG_LEVEL").unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_string());

        let environment = match get("ARTHA_ENV") {
            None => Environment::Development,
            Some(raw) => match raw.to_ascii_lowercase().as_str() {
                "development" | "dev" => Environment::Development,
                "production" | "prod" => Environment::Production,
                _ => return Err(ConfigError::InvalidEnvironment(raw)),
            },
        };

        // Resolve eagerly so a typo'd host fails fast at startup rather than
        // on the first incoming connection attempt.
        let _ = format!("{host}:{port}")
            .to_socket_addrs()
            .map_err(|_| ConfigError::InvalidHost(host.clone()))?;

        Ok(Config { host, port, db_path, log_level, environment })
    }

    /// The address to bind the HTTP listener to.
    pub fn socket_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .to_socket_addrs()
            .expect("host was already validated in from_lookup")
            .next()
            .expect("to_socket_addrs returned no addresses")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + '_ {
        let map: HashMap<&str, &str> = vars.iter().copied().collect();
        move |key| map.get(key).map(|v| v.to_string())
    }

    #[test]
    fn defaults_when_nothing_set() {
        let cfg = Config::from_lookup(lookup(&[])).unwrap();
        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 8080);
        assert_eq!(cfg.db_path, PathBuf::from("data/artha.db"));
        assert_eq!(cfg.log_level, "info");
        assert_eq!(cfg.environment, Environment::Development);
    }

    #[test]
    fn overrides_from_env() {
        let cfg = Config::from_lookup(lookup(&[
            ("ARTHA_HOST", "127.0.0.1"),
            ("ARTHA_PORT", "9090"),
            ("ARTHA_DB_PATH", "custom/path.db"),
            ("ARTHA_LOG_LEVEL", "debug"),
            ("ARTHA_ENV", "production"),
        ]))
        .unwrap();
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.port, 9090);
        assert_eq!(cfg.db_path, PathBuf::from("custom/path.db"));
        assert_eq!(cfg.log_level, "debug");
        assert_eq!(cfg.environment, Environment::Production);
    }

    #[test]
    fn environment_accepts_short_aliases() {
        assert_eq!(Config::from_lookup(lookup(&[("ARTHA_ENV", "dev")])).unwrap().environment, Environment::Development);
        assert_eq!(Config::from_lookup(lookup(&[("ARTHA_ENV", "prod")])).unwrap().environment, Environment::Production);
        assert_eq!(
            Config::from_lookup(lookup(&[("ARTHA_ENV", "PRODUCTION")])).unwrap().environment,
            Environment::Production
        );
    }

    #[test]
    fn rejects_invalid_port() {
        let err = Config::from_lookup(lookup(&[("ARTHA_PORT", "not-a-port")])).unwrap_err();
        assert_eq!(err, ConfigError::InvalidPort("not-a-port".to_string()));
    }

    #[test]
    fn rejects_port_out_of_u16_range() {
        let err = Config::from_lookup(lookup(&[("ARTHA_PORT", "99999")])).unwrap_err();
        assert!(matches!(err, ConfigError::InvalidPort(_)));
    }

    #[test]
    fn rejects_invalid_environment() {
        let err = Config::from_lookup(lookup(&[("ARTHA_ENV", "staging")])).unwrap_err();
        assert_eq!(err, ConfigError::InvalidEnvironment("staging".to_string()));
    }

    #[test]
    fn socket_addr_combines_host_and_port() {
        let cfg = Config::from_lookup(lookup(&[("ARTHA_HOST", "127.0.0.1"), ("ARTHA_PORT", "9090")])).unwrap();
        assert_eq!(cfg.socket_addr().to_string(), "127.0.0.1:9090");
    }
}
