//! Append-only audit log for sensitive actions (owner/staff admin, backups,
//! restores, updates — see spec §11, §14). Deliberately dumb: one row per
//! action, free-form JSON detail blob. Reporting/filtering comes later.

use rusqlite::Connection;

use crate::error::ApiError;
use crate::ids::{new_id, now_iso};

pub fn record(
    conn: &Connection,
    business_id: &str,
    user_id: Option<&str>,
    action: &str,
    entity_type: &str,
    entity_id: Option<&str>,
    detail_json: &str,
) -> Result<(), ApiError> {
    conn.execute(
        "INSERT INTO audit_log (audit_id, business_id, user_id, action, entity_type, entity_id, detail_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            new_id(),
            business_id,
            user_id,
            action,
            entity_type,
            entity_id,
            detail_json,
            now_iso(),
        ],
    )?;
    Ok(())
}
