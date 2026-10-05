//! Audit-friendly identifiers and timestamps.
//!
//! Every entity (business, user, product, purchase, sale, audit row, ...)
//! gets a UUIDv4 primary key assigned by the application at creation time,
//! not an autoincrement rowid. That keeps IDs stable across export/import,
//! backup/restore, and future multi-location sync, and means a transaction's
//! ID can be printed on a bill before the DB write even commits.

use uuid::Uuid;

/// Generate a new random (v4) ID as a lowercase hyphenated string.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// Current UTC time as an ISO-8601 string, e.g. `2026-10-05T12:34:56.789Z`.
/// Stored as TEXT in SQLite so it stays human-readable in ad-hoc inspection.
pub fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    // Avoid pulling in `chrono` for one timestamp format; a tiny manual
    // RFC3339 formatter keeps the dependency tree small for V0.
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_unix(dur.as_secs(), dur.subsec_millis())
}

/// Exposed for `auth::add_hours_iso`, which needs to format an arbitrary
/// future `(secs, millis)` pair the same way `now_iso` formats the present.
pub fn format_unix_public(secs: u64, millis: u32) -> String {
    format_unix(secs, millis)
}

fn format_unix(secs: u64, millis: u32) -> String {
    // Civil-from-days algorithm (Howard Hinnant's), good for any date we
    // care about here, with no external crate.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_ <= 2 { y + 1 } else { y };

    format!(
        "{y:04}-{m_:02}-{d:02}T{h:02}:{m:02}:{s:02}.{millis:03}Z"
    )
}
