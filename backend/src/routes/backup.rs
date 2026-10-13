//! Local backup creation, listing, verification, retention, and restore
//! (Phase 09 / V0.3 Reliability, spec §9, §10).
//!
//! Backups are taken with SQLite's online backup API directly against the
//! single shared `Connection` in `AppState`, so a backup is a consistent
//! snapshot even while the app is handling requests, with no need to pause
//! the server or copy a possibly-mid-write file by hand.
//!
//! Restore uses the same API in reverse, copying a backup file's pages
//! directly into the live, already-open `Connection` rather than swapping
//! database files on disk out from under a process that holds file handles
//! and WAL state on the original. This avoids the file-lock/interruption
//! hazards of a raw file copy entirely; see `restore_backup_handler` for the
//! ordered safety checks (confirm -> verify target -> safety-backup current
//! state -> restore -> re-migrate -> post-restore integrity check).
//!
//! Owner-only per spec §11 ("Owner-only access to ... backups, restores").

use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::extract::{Path as AxPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth::AuthUser;
use crate::error::ApiError;
use crate::ids::new_id;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/api/backups",
            get(list_backups).post(create_backup_handler),
        )
        .route("/api/backups/:id/verify", post(verify_backup_handler))
        .route("/api/backups/:id/restore", post(restore_backup_handler))
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupView {
    pub backup_id: String,
    pub app_version: String,
    pub schema_version: i64,
    pub path: String,
    pub checksum_sha256: String,
    pub created_at: String,
    pub size_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct RestoreRequest {
    pub confirm: bool,
}

struct VerifyResult {
    checksum_ok: bool,
    integrity_ok: bool,
    schema_version: Option<i64>,
}

fn current_schema_version(conn: &Connection) -> Result<i64, ApiError> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?)
}

/// Core backup routine, shared by the HTTP handler and the scheduled
/// background task in `main.rs`. Not `async`: it does no awaiting, only
/// synchronous file and SQLite work while briefly holding the shared db
/// mutex, exactly like every other handler in this codebase.
pub fn create_backup(state: &AppState) -> Result<BackupView, ApiError> {
    std::fs::create_dir_all(&state.backup_dir)
        .map_err(|e| ApiError::Internal(format!("failed to create backup dir: {e}")))?;

    let backup_id = new_id();
    let now = crate::ids::now_iso();
    let dest_path: PathBuf = state
        .backup_dir
        .join(format!("artha-backup-{backup_id}.sqlite3"));

    let schema_version = {
        let conn = state.db.lock().expect("db mutex poisoned");
        let schema_version = current_schema_version(&conn)?;

        // Destination is a brand-new plain file; scoped so it (and its file
        // handle) is closed before we read it back for the checksum below.
        {
            let mut dest_conn = Connection::open(&dest_path)
                .map_err(|e| ApiError::Internal(format!("failed to create backup file: {e}")))?;
            let backup = Backup::new(&conn, &mut dest_conn)
                .map_err(|e| ApiError::Internal(format!("backup init failed: {e}")))?;
            backup
                .run_to_completion(100, Duration::from_millis(50), None)
                .map_err(|e| ApiError::Internal(format!("backup failed: {e}")))?;
        }
        schema_version
    };

    let bytes = std::fs::read(&dest_path)
        .map_err(|e| ApiError::Internal(format!("failed to read backup for checksum: {e}")))?;
    let checksum_sha256 = hex::encode(Sha256::digest(&bytes));
    let size_bytes = bytes.len() as u64;
    let app_version = env!("CARGO_PKG_VERSION").to_string();
    let path = dest_path.to_string_lossy().into_owned();

    {
        let conn = state.db.lock().expect("db mutex poisoned");
        conn.execute(
            "INSERT INTO backups (backup_id, app_version, schema_version, path, checksum_sha256, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![backup_id, app_version, schema_version, path, checksum_sha256, now],
        )?;
    }

    prune_old_backups(state)?;

    Ok(BackupView {
        backup_id,
        app_version,
        schema_version,
        path,
        checksum_sha256,
        created_at: now,
        size_bytes,
    })
}

/// Keeps the most recent `backup_retention` backups (by `created_at`),
/// deleting older rows and their files. Always keeps at least 1 regardless
/// of configuration, so a misconfigured `ARTHA_BACKUP_RETENTION=0` can never
/// delete the backup that was just taken.
fn prune_old_backups(state: &AppState) -> Result<(), ApiError> {
    let effective_retention = state.backup_retention.max(1);
    let conn = state.db.lock().expect("db mutex poisoned");

    let mut stmt = conn.prepare("SELECT backup_id, path FROM backups ORDER BY created_at DESC")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    for (backup_id, path) in rows.into_iter().skip(effective_retention) {
        // Best-effort file removal; the database row is the source of truth
        // for what "exists" means, so a stray orphaned file on disk is a
        // harmless leftover rather than a correctness problem.
        let _ = std::fs::remove_file(&path);
        conn.execute(
            "DELETE FROM backups WHERE backup_id = ?1",
            rusqlite::params![backup_id],
        )?;
    }

    Ok(())
}

/// Checks a backup file's checksum, SQLite integrity, and records its own
/// schema version — all read-only, never touching the live database.
fn verify_backup_file(path: &Path, expected_checksum: &str) -> Result<VerifyResult, ApiError> {
    let bytes = std::fs::read(path)
        .map_err(|e| ApiError::NotFound(format!("backup file missing or unreadable: {e}")))?;
    let actual_checksum = hex::encode(Sha256::digest(&bytes));
    let checksum_ok = actual_checksum == expected_checksum;

    let (integrity_ok, schema_version) =
        match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => {
                let integrity: String = conn
                    .query_row("PRAGMA integrity_check", [], |r| r.get(0))
                    .unwrap_or_else(|_| "error".to_string());
                let schema_version: Option<i64> = current_schema_version(&conn).ok();
                (integrity == "ok", schema_version)
            }
            Err(_) => (false, None),
        };

    Ok(VerifyResult {
        checksum_ok,
        integrity_ok,
        schema_version,
    })
}

async fn list_backups(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<BackupView>>, ApiError> {
    user.require_owner()?;
    let conn = state.db.lock().expect("db mutex poisoned");
    let mut stmt = conn.prepare(
        "SELECT backup_id, app_version, schema_version, path, checksum_sha256, created_at \
         FROM backups ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (backup_id, app_version, schema_version, path, checksum_sha256, created_at) = row?;
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        out.push(BackupView {
            backup_id,
            app_version,
            schema_version,
            path,
            checksum_sha256,
            created_at,
            size_bytes,
        });
    }

    Ok(Json(out))
}

async fn create_backup_handler(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<BackupView>, ApiError> {
    user.require_owner()?;
    let view = create_backup(&state)?;

    let conn = state.db.lock().expect("db mutex poisoned");
    crate::audit::record(
        &conn,
        &user.business_id,
        Some(&user.user_id),
        "backup_created",
        "backup",
        Some(&view.backup_id),
        &serde_json::json!({ "backup_id": view.backup_id, "size_bytes": view.size_bytes })
            .to_string(),
    )?;

    Ok(Json(view))
}

async fn verify_backup_handler(
    State(state): State<AppState>,
    user: AuthUser,
    AxPath(id): AxPath<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    user.require_owner()?;

    let (path, checksum): (String, String) = {
        let conn = state.db.lock().expect("db mutex poisoned");
        conn.query_row(
            "SELECT path, checksum_sha256 FROM backups WHERE backup_id = ?1",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| ApiError::NotFound("Backup not found".into()))?
    };

    let result = verify_backup_file(Path::new(&path), &checksum)?;

    Ok(Json(serde_json::json!({
        "backup_id": id,
        "checksum_ok": result.checksum_ok,
        "integrity_ok": result.integrity_ok,
        "schema_version": result.schema_version,
        "valid": result.checksum_ok && result.integrity_ok,
    })))
}

async fn restore_backup_handler(
    State(state): State<AppState>,
    user: AuthUser,
    AxPath(id): AxPath<String>,
    Json(req): Json<RestoreRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    user.require_owner()?;

    if !req.confirm {
        return Err(ApiError::BadRequest(
            "Restore requires explicit confirmation (confirm: true)".into(),
        ));
    }

    let restored_row: BackupView = {
        let conn = state.db.lock().expect("db mutex poisoned");
        conn.query_row(
            "SELECT backup_id, app_version, schema_version, path, checksum_sha256, created_at \
             FROM backups WHERE backup_id = ?1",
            rusqlite::params![id],
            |r| {
                let path: String = r.get(3)?;
                Ok(BackupView {
                    backup_id: r.get(0)?,
                    app_version: r.get(1)?,
                    schema_version: r.get(2)?,
                    path,
                    checksum_sha256: r.get(4)?,
                    created_at: r.get(5)?,
                    size_bytes: 0,
                })
            },
        )
        .map_err(|_| ApiError::NotFound("Backup not found".into()))?
    };
    let path = restored_row.path.clone();
    let checksum = restored_row.checksum_sha256.clone();
    let backup_schema_version = restored_row.schema_version;

    // 1. Validate the target backup before touching anything live: checksum,
    // SQLite integrity, and schema compatibility.
    let verify = verify_backup_file(Path::new(&path), &checksum)?;
    if !verify.checksum_ok {
        return Err(ApiError::BadRequest(
            "Backup checksum mismatch; refusing to restore a possibly corrupted or tampered backup"
                .into(),
        ));
    }
    if !verify.integrity_ok {
        return Err(ApiError::BadRequest(
            "Backup failed SQLite integrity check; refusing to restore".into(),
        ));
    }

    let live_schema_version = {
        let conn = state.db.lock().expect("db mutex poisoned");
        current_schema_version(&conn)?
    };

    if backup_schema_version > live_schema_version {
        return Err(ApiError::Conflict(format!(
            "Backup schema version ({backup_schema_version}) is newer than this application's \
             schema version ({live_schema_version}); refusing to restore data from a newer app version"
        )));
    }

    // 2. Safety backup of current live state, taken before any destructive
    // action, per spec §10 ("create a safety backup of the current state").
    let safety_backup = create_backup(&state)?;

    // 3. Restore: copy the backup file's pages into the live, already-open
    // connection via SQLite's backup API (see module docs for why not a
    // plain file swap).
    let restore_result: Result<(), ApiError> = (|| {
        let backup_conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| ApiError::Internal(format!("failed to open backup for restore: {e}")))?;
        let mut live = state.db.lock().expect("db mutex poisoned");
        let backup = Backup::new(&backup_conn, &mut live)
            .map_err(|e| ApiError::Internal(format!("restore init failed: {e}")))?;
        backup
            .run_to_completion(100, Duration::from_millis(50), None)
            .map_err(|e| ApiError::Internal(format!("restore failed: {e}")))?;
        Ok(())
    })();

    if let Err(e) = restore_result {
        return Err(ApiError::Internal(format!(
            "restore failed ({e:?}); the live database may be inconsistent. A safety backup taken \
             immediately before this attempt is available (backup_id: {}); restore from it to recover.",
            safety_backup.backup_id
        )));
    }

    // 4. A restored backup from an older schema version now needs the
    // binary's newer migrations re-applied; this is a no-op when the
    // versions already match (see db::run_migrations's idempotency tests).
    {
        let live = state.db.lock().expect("db mutex poisoned");
        crate::db::run_migrations(&live)?;
    }

    // 5. Post-restore integrity check on the now-live database, per spec
    // §10 ("run integrity checks ... after restore").
    let post_integrity: String = {
        let live = state.db.lock().expect("db mutex poisoned");
        live.query_row("PRAGMA integrity_check", [], |r| r.get(0))?
    };
    if post_integrity != "ok" {
        return Err(ApiError::Internal(format!(
            "restore completed but post-restore integrity check reported '{post_integrity}'; \
             restore from the safety backup (backup_id: {}) immediately",
            safety_backup.backup_id
        )));
    }

    // 6. The database we just restored is a full snapshot, including its own
    // `backups` table as it looked at the moment that snapshot was taken —
    // which predates both the row for the backup just restored-from (if it
    // was the very first backup ever taken: its own row is inserted *after*
    // its file is written) and the safety backup taken in step 2 (which by
    // definition did not exist yet at snapshot time either). Without this,
    // the safety backup — the one thing meant to let the owner undo this
    // restore — would be invisible to `list_backups` even though its file
    // is sitting right there on disk. Re-assert both rows; `OR IGNORE`
    // because the snapshot may already contain one or both if it was taken
    // later than their creation.
    {
        let conn = state.db.lock().expect("db mutex poisoned");
        for row in [&restored_row, &safety_backup] {
            conn.execute(
                "INSERT OR IGNORE INTO backups (backup_id, app_version, schema_version, path, checksum_sha256, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    row.backup_id,
                    row.app_version,
                    row.schema_version,
                    row.path,
                    row.checksum_sha256,
                    row.created_at
                ],
            )?;
        }
    }

    {
        let conn = state.db.lock().expect("db mutex poisoned");
        crate::audit::record(
            &conn,
            &user.business_id,
            Some(&user.user_id),
            "backup_restored",
            "backup",
            Some(&id),
            &serde_json::json!({
                "restored_backup_id": id,
                "safety_backup_id": safety_backup.backup_id,
            })
            .to_string(),
        )?;
    }

    Ok(Json(serde_json::json!({
        "status": "restored",
        "restored_backup_id": id,
        "safety_backup_id": safety_backup.backup_id,
    })))
}
