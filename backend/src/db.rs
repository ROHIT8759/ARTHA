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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A unique path per test under the OS temp dir, so parallel test
    /// threads (the default `cargo test` behavior) never collide on the
    /// same SQLite file.
    fn temp_db_path(label: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("artha_db_test_{label}_{}_{n}.db", std::process::id()))
    }

    struct TempFile(std::path::PathBuf);
    impl Drop for TempFile {
        fn drop(&mut self) {
            for ext in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
            }
        }
    }

    #[test]
    fn open_and_migrate_creates_and_records_schema_version() {
        let path = temp_db_path("migrate_records_version");
        let _cleanup = TempFile(path.clone());

        let conn = open_and_migrate(&path).expect("open_and_migrate should succeed");

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0))
            .expect("schema_migrations should have at least one row");
        assert_eq!(version, 1);

        // A core table from migration 0001 should exist and be queryable.
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn reopening_an_already_migrated_db_does_not_reapply_migrations() {
        let path = temp_db_path("migrate_idempotent");
        let _cleanup = TempFile(path.clone());

        {
            let conn = open_and_migrate(&path).unwrap();
            conn.execute(
                "INSERT INTO businesses (business_id, name, settings_json, created_at, updated_at) VALUES ('b1', 'Test', '{}', 't', 't')",
                [],
            )
            .unwrap();
        } // connection dropped, file persists

        let conn = open_and_migrate(&path).expect("reopening an existing db should succeed");

        let migration_rows: i64 = conn.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(migration_rows, 1, "migration 1 should be recorded exactly once");

        // Pre-existing data survives a reopen (this would also fail loudly
        // if run_migrations ever re-ran the CREATE TABLE statements without
        // IF NOT EXISTS and wiped the table).
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM businesses", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn open_and_migrate_creates_parent_directories() {
        let path = temp_db_path("nested/does_not_exist_yet");
        let dir_to_clean = path.parent().unwrap().to_path_buf();

        let conn = open_and_migrate(&path).expect("should create missing parent directories");
        assert!(path.exists());

        // Drop the connection (and its WAL/SHM sidecar files' locks) before
        // removing the whole directory in one go, rather than relying on
        // Drop-order interleaving with a separate cleanup guard.
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir_to_clean);
    }

    #[test]
    fn open_in_memory_is_already_migrated() {
        let conn = open_in_memory();
        let version: i64 = conn.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 1);
    }
}
