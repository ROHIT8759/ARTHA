//! A single error type for the whole API, with a `From` conversion to an
//! Axum response so handlers can just `?` and get a sane HTTP status + JSON
//! body instead of panicking or leaking internals to the client.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    PayloadTooLarge(String),
    Internal(String),
    /// A destructive operation (currently: backup restore) failed *after* a
    /// mandatory safety backup was already taken. Unlike `Internal`, this
    /// variant's message is deliberately NOT sanitized away: the operator
    /// needs the safety-backup id to recover, so the client-visible message
    /// is limited to a short, pre-written, safe description plus that id —
    /// never the raw underlying error, a file path, or a stack trace. Full
    /// diagnostic detail still goes to the server trace log at the call
    /// site before this variant is constructed.
    RestoreFailed {
        client_message: String,
        safety_backup_id: String,
    },
}

impl ApiError {
    fn status_and_message(&self) -> (StatusCode, &str) {
        match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::PayloadTooLarge(m) => (StatusCode::PAYLOAD_TOO_LARGE, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
            // Never actually reached: `into_response` intercepts this variant
            // before calling `status_and_message`. Kept so the match stays
            // exhaustive (no `_ =>` catch-all to silently absorb a future
            // variant) rather than because this arm's message is used.
            ApiError::RestoreFailed { .. } => (StatusCode::INTERNAL_SERVER_ERROR, "restore_failed"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::RestoreFailed {
            client_message,
            safety_backup_id,
        } = &self
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "restore_failed",
                    "message": client_message,
                    "safety_backup_id": safety_backup_id,
                })),
            )
                .into_response();
        }

        let (status, message) = self.status_and_message();
        if status == StatusCode::INTERNAL_SERVER_ERROR {
            // Internal details go to the server log, not the client.
            tracing::error!(%message, "internal error");
            return (
                status,
                Json(json!({ "error": "internal_error", "message": "Something went wrong." })),
            )
                .into_response();
        }
        (
            status,
            Json(json!({ "error": status.as_str(), "message": message })),
        )
            .into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Internal(format!("db error: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `RestoreFailed`'s two real construction sites in `routes::backup`
    /// (restore-copy failure, post-integrity-check failure) proved
    /// impractical to trigger deterministically through the HTTP/integration
    /// test harness: both failure branches sit after the exact same file has
    /// already passed verification moments earlier in the same request, with
    /// no fault-injection hook to force a failure in between. This test
    /// instead verifies the response *contract* directly: given this
    /// variant, the client must receive the safety-backup id and a safe
    /// message, and never the sanitized generic "Something went wrong."
    /// that every other `Internal` error produces.
    #[tokio::test]
    async fn restore_failed_response_includes_safety_backup_id_and_is_not_sanitized() {
        let err = ApiError::RestoreFailed {
            client_message:
                "Restore completed but the database failed a post-restore integrity check."
                    .to_string(),
            safety_backup_id: "safety-123".to_string(),
        };

        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(body["error"], "restore_failed");
        assert_eq!(body["safety_backup_id"], "safety-123");
        assert_eq!(
            body["message"],
            "Restore completed but the database failed a post-restore integrity check."
        );
        // The defining property this variant exists for: this message must
        // never be replaced with the generic sanitized string.
        assert_ne!(body["message"], "Something went wrong.");
    }

    #[tokio::test]
    async fn ordinary_internal_errors_are_still_sanitized() {
        // Regression guard: adding `RestoreFailed`'s special-cased branch in
        // `into_response` must not accidentally change `Internal`'s existing
        // sanitization behavior for every other unrelated server error.
        let err = ApiError::Internal("some raw db path or stack detail".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"], "internal_error");
        assert_eq!(body["message"], "Something went wrong.");
        assert_ne!(body["message"], "some raw db path or stack detail");
    }
}
