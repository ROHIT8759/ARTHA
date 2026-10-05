//! SQLite connection setup and migrations.
//!
//! V0 ships schema as a single baked-in migration file (`migrations/0001_init.sql`).
//! As the schema grows, add `0002_*.sql`, etc., and extend `run_migrations` to
//! apply whichever ones a given database hasn't seen yet, tracked in
//! `schema_migrations`. Keeping that bookkeeping table from migration #1
//! avoids an awkward "add it later" step once real data exists.

use std::path::Path;

use rusqlite::Connection;

use crate::error::ApiError;

/// Open (creating if necessary) the SQLite database at `path`, apply
/// pragmas needed for correctness/durability, and run any pending
/// migrations.
pub fn open_and_migrate(path: &Path) -> Result<Connection, ApiError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| ApiError::Internal(format!("failed to create db dir: {e}")))?;
    }

    let conn = Connection::open(path)
        .map_err(|e| ApiError::Internal(format!("failed to open db: {e}")))?;

    // WAL + synchronous=NORMAL is the standard "durable enough, fast enough"
    // combo for a single-writer local app; foreign_keys is off by default
    // in SQLite and must be turned on per-connection.
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;

    run_migrations(&conn)?;
    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<(), ApiError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            applied_at  TEXT NOT NULL
        );",
    )?;

    let applied: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // (version, sql) pairs, in order. `include_str!` bakes the migration
    // into the binary so deployment is just copying one executable.
    let migrations: &[(i64, &str)] = &[(1, include_str!("../migrations/0001_init.sql"))];

    for (version, sql) in migrations {
        if *version > applied {
            conn.execute_batch(sql)?;
            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                rusqlite::params![version, crate::ids::now_iso()],
            )?;
            tracing::info!(version, "applied migration");
        }
    }

    Ok(())
}

/// An in-memory, already-migrated database. Used by both this module's own
/// unit tests and the integration tests under `tests/` (which link against
/// a normal, non-`cfg(test)` build of this crate — a `#[cfg(test)]` guard
/// here would make the function disappear for them, not just for release
/// builds).
pub fn open_in_memory() -> Connection {
    let conn = Connection::open_in_memory().expect("open in-memory sqlite");
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    run_migrations(&conn).expect("run migrations");
    conn
}
