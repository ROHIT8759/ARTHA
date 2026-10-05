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
    Internal(String),
}

impl ApiError {
    fn status_and_message(&self) -> (StatusCode, &str) {
        match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            ApiError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            ApiError::Conflict(m) => (StatusCode::CONFLICT, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
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
        (status, Json(json!({ "error": status.as_str(), "message": message }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Internal(format!("db error: {e}"))
    }
}
