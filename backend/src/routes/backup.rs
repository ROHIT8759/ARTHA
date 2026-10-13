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
    create_backup_preserving(state, &[])
}

/// Same as `create_backup`, but the retention prune that follows will never
/// delete a backup whose id appears in `preserve_ids` — regardless of how
/// old it is — even if every other backup would otherwise be eligible.
/// Used by restore so that taking the mandatory safety backup can never
/// delete the very backup being restored (see the production-safety review,
/// Finding 1).
fn create_backup_preserving(
    state: &AppState,
    preserve_ids: &[&str],
) -> Result<BackupView, ApiError> {
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

    prune_old_backups(state, preserve_ids)?;

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

/// Keeps the most recent `backup_retention` non-preserved backups (by
/// `created_at`), deleting older rows and their files. Always keeps at
/// least 1 regardless of configuration, so a misconfigured
/// `ARTHA_BACKUP_RETENTION=0` can never delete the backup that was just
/// taken. Anything in `preserve_ids` is kept unconditionally and does not
/// consume a slot in the retention budget — it can push the total count
/// above `backup_retention`, which is intentional: we never force-delete a
/// backup we were explicitly told to keep just to hit a count target.
fn prune_old_backups(state: &AppState, preserve_ids: &[&str]) -> Result<(), ApiError> {
    let effective_retention = state.backup_retention.max(1);
    let conn = state.db.lock().expect("db mutex poisoned");

    let mut stmt = conn.prepare("SELECT backup_id, path FROM backups ORDER BY created_at DESC")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let mut kept = 0usize;
    for (backup_id, path) in rows {
        if preserve_ids.contains(&backup_id.as_str()) {
            continue;
        }
        if kept < effective_retention {
            kept += 1;
            continue;
        }
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

/// Reads every backup row currently in the live `backups` table. Used by
/// restore to remember, before the destructive copy overwrites that table
/// with an older snapshot's own version of it, exactly which backups validly
/// existed a moment ago — so they can all be re-asserted afterward, not just
/// the two rows directly involved in the restore itself (see the
/// production-safety review, Finding 4). This only re-reads rows that were
/// already present in the live table a moment earlier; it never re-verifies
/// or re-registers arbitrary files, so nothing unverified can be newly
/// introduced by the re-assertion step.
fn snapshot_all_backup_rows(state: &AppState) -> Result<Vec<BackupView>, ApiError> {
    let conn = state.db.lock().expect("db mutex poisoned");
    let mut stmt = conn.prepare(
        "SELECT backup_id, app_version, schema_version, path, checksum_sha256, created_at FROM backups",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(BackupView {
            backup_id: r.get(0)?,
            app_version: r.get(1)?,
            schema_version: r.get(2)?,
            path: r.get(3)?,
            checksum_sha256: r.get(4)?,
            created_at: r.get(5)?,
            size_bytes: 0,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
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

    // 1. Validate the target backup before touching anything live: checksum,
    // SQLite integrity, and schema compatibility. The compatibility decision
    // uses `verify.schema_version` — independently re-derived by opening the
    // just-checksummed, just-integrity-checked file itself — rather than the
    // `schema_version` column on the `backups` row. That column isn't
    // protected by the checksum (which covers only the backup file's bytes),
    // so basing the safety decision on the verified file keeps the whole
    // compatibility gate derived from data that was actually just verified.
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
    let verified_backup_schema_version = verify.schema_version.unwrap_or(0);

    let live_schema_version = {
        let conn = state.db.lock().expect("db mutex poisoned");
        current_schema_version(&conn)?
    };

    if verified_backup_schema_version > live_schema_version {
        return Err(ApiError::Conflict(format!(
            "Backup schema version ({verified_backup_schema_version}) is newer than this application's \
             schema version ({live_schema_version}); refusing to restore data from a newer app version"
        )));
    }

    // 2. Safety backup of current live state, taken before any destructive
    // action, per spec §10 ("create a safety backup of the current state").
    // The restore target (`id`) is explicitly preserved from this backup's
    // own retention pruning, so taking the safety backup can never delete
    // the very backup about to be restored from (production-safety review,
    // Finding 1).
    let safety_backup = create_backup_preserving(&state, &[&id])?;

    // Snapshot every backup row that validly exists right now — after the
    // safety backup and its retention pruning, so this reflects reality, not
    // a stale pre-prune view. The restore below overwrites the whole
    // `backups` table with an older snapshot's own version of it; without
    // this, any backup not part of that older snapshot (including the
    // restore target itself and the safety backup just taken) would vanish
    // from `list_backups` even though its file remains untouched on disk
    // (production-safety review, Finding 4). This only re-reads rows that
    // already validly existed a moment ago, so it can't register anything
    // unverified as a backup.
    let pre_restore_backups = snapshot_all_backup_rows(&state)?;

    // 3. Restore the backup file's pages into the live, already-open
    // connection via SQLite's backup API (see module docs for why not a
    // plain file swap), then immediately re-apply migrations and run the
    // post-restore integrity check — all three under one continuous lock
    // hold. Releasing and reacquiring the lock between these steps would
    // open a window where a concurrent request could execute against a
    // database that was just restored but not yet brought up to the
    // running binary's expected schema (production-safety review,
    // Finding 5). Restore is a rare, explicit, owner-only action, so
    // holding the lock slightly longer here is an accepted trade-off, not
    // a change to any other endpoint's concurrency behavior.
    let restore_outcome: Result<String, ApiError> = (|| {
        let backup_conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| ApiError::Internal(format!("failed to open backup for restore: {e}")))?;
        let mut live = state.db.lock().expect("db mutex poisoned");

        let backup = Backup::new(&backup_conn, &mut live)
            .map_err(|e| ApiError::Internal(format!("restore init failed: {e}")))?;
        backup
            .run_to_completion(100, Duration::from_millis(50), None)
            .map_err(|e| ApiError::Internal(format!("restore failed: {e}")))?;
        // Explicitly finalize and drop the `Backup` handle (which holds a
        // mutable borrow of `live`) now that the copy is complete, so `live`
        // can be used directly below for migrations and the integrity check
        // without releasing its mutex guard in between.
        drop(backup);

        // 4. A restored backup from an older schema version now needs the
        // binary's newer migrations re-applied; this is a no-op when the
        // versions already match (see db::run_migrations's idempotency
        // tests). Still holding `live`'s lock, so no other request can
        // observe the gap between "restored" and "migrated".
        crate::db::run_migrations(&live)?;

        // 5. Post-restore integrity check on the now-live database, per
        // spec §10 ("run integrity checks ... after restore").
        let post_integrity: String = live.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        Ok(post_integrity)
    })();

    let post_integrity = match restore_outcome {
        Ok(status) => status,
        Err(e) => {
            tracing::error!(
                error = ?e,
                restored_backup_id = %id,
                safety_backup_id = %safety_backup.backup_id,
                "restore-copy or re-migration failed"
            );
            return Err(ApiError::RestoreFailed {
                client_message:
                    "The restore operation did not complete. Your pre-restore data was \
                    not lost — a safety backup was taken immediately before this attempt."
                        .to_string(),
                safety_backup_id: safety_backup.backup_id,
            });
        }
    };

    if post_integrity != "ok" {
        tracing::error!(
            post_integrity = %post_integrity,
            restored_backup_id = %id,
            safety_backup_id = %safety_backup.backup_id,
            "post-restore integrity check failed"
        );
        return Err(ApiError::RestoreFailed {
            client_message: "Restore completed but the database failed a post-restore integrity \
                check. Restore the safety backup listed here immediately."
                .to_string(),
            safety_backup_id: safety_backup.backup_id,
        });
    }

    // 6. Re-assert every backup row captured just before the destructive
    // restore (see the comment above `pre_restore_backups`). `OR IGNORE`
    // because the restored snapshot may already contain some of them if it
    // was taken later than their creation.
    {
        let conn = state.db.lock().expect("db mutex poisoned");
        for row in &pre_restore_backups {
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

    // A restore can replace the `businesses`/`users`/`sessions` tables
    // wholesale (e.g. restoring a backup taken before `/api/setup` ever
    // ran). The currently authenticated owner's `business_id`/`user_id`
    // were captured by the `AuthUser` extractor *before* this handler ran,
    // against the pre-restore tables — they may no longer exist. Check this
    // explicitly so the response can tell the caller plainly, rather than
    // leaving them to discover it only when their next request 401s.
    let current_session_still_valid: bool = {
        let conn = state.db.lock().expect("db mutex poisoned");
        let business_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM businesses WHERE business_id = ?1)",
                rusqlite::params![user.business_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let user_exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM users WHERE user_id = ?1)",
                rusqlite::params![user.user_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        business_exists != 0 && user_exists != 0
    };

    // The restore itself has already fully succeeded by this point. A
    // restored database whose business/user rows predate the current
    // session can make this specific, best-effort audit write fail its
    // foreign-key check — that must never be reported back as "the restore
    // failed"; it's purely a bookkeeping gap on an otherwise-successful
    // operation (production-safety review, Finding 3).
    {
        let conn = state.db.lock().expect("db mutex poisoned");
        if let Err(e) = crate::audit::record(
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
        ) {
            tracing::warn!(
                error = ?e,
                restored_backup_id = %id,
                "post-restore audit entry could not be written (restore itself still succeeded)"
            );
        }
    }

    Ok(Json(serde_json::json!({
        "status": "restored",
        "restored_backup_id": id,
        "safety_backup_id": safety_backup.backup_id,
        "current_session_still_valid": current_session_still_valid,
    })))
}
