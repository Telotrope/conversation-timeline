//! Maps this crate's port errors (and route-level conditions like "not
//! found") to HTTP responses. Every branch is a distinct, logged case --
//! per CLAUDE.md's exception-handling rule, a backend error is never
//! swallowed into a generic 500 without at least being visible in the
//! response body's error code.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use timeline_core::ports::errors::{ObjectStoreError, StoreError};

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    /// The request itself is wrong (a missing or unknown field, nothing to
    /// change). The message is sent back so the caller can fix it.
    BadRequest(String),
    /// Well-formed, but not allowed -- e.g. a flag save whose handle doesn't
    /// match the message it names.
    Forbidden(String),
    Store(StoreError),
    ObjectStore(ObjectStoreError),
    /// A server-side condition that isn't a storage-backend failure at all
    /// -- e.g. previously-validated data failing to re-parse on export, or
    /// a dev-only token-signing failure. Distinct from `Store`/`ObjectStore`
    /// so a real backend outage is never conflated with "something we
    /// wrote or generated ourselves turned out to be broken."
    Internal(String),
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError::NotFound,
            other => ApiError::Store(other),
        }
    }
}

impl From<ObjectStoreError> for ApiError {
    fn from(e: ObjectStoreError) -> Self {
        match e {
            ObjectStoreError::NotFound => ApiError::NotFound,
            other => ApiError::ObjectStore(other),
        }
    }
}

const MAX_ECHOED_CHARS: usize = 300;

fn bounded(message: &str) -> String {
    if message.chars().count() > MAX_ECHOED_CHARS {
        format!(
            "{}…",
            message.chars().take(MAX_ECHOED_CHARS).collect::<String>()
        )
    } else {
        message.to_string()
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    /// Set only for stored data that can't be read (`"data_integrity"`), so
    /// the page can tell a failure that trying again won't fix from one it
    /// might (plan 2026-10-06-load-only-what-the-page-shows.md §12.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<&'static str>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // The underlying cause is deliberately not echoed to the client (it
        // may name internal details like a table name), but it is not
        // dropped either -- logged to stderr, which Lambda ships to
        // CloudWatch Logs automatically. A real deployment would want a
        // structured `tracing` subscriber instead of a raw eprintln; this is
        // the minimal version that still satisfies CLAUDE.md's rule that an
        // error must be visible somewhere, not silently discarded.
        let mut error_kind = None;
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            // These messages can quote text from the request (serde names an
            // unknown field), so they're cut to a bounded length before being
            // echoed back. JSON serialization escapes the rest.
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, bounded(m)),
            ApiError::Forbidden(m) => (StatusCode::FORBIDDEN, bounded(m)),
            ApiError::Store(e @ StoreError::Damaged(_)) => {
                eprintln!("{e}");
                error_kind = Some("data_integrity");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "stored data can't be read".to_string(),
                )
            }
            ApiError::Store(e) => {
                eprintln!("storage backend error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage backend error".to_string(),
                )
            }
            ApiError::ObjectStore(e) => {
                eprintln!("object store backend error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "object store backend error".to_string(),
                )
            }
            ApiError::Internal(e) => {
                eprintln!("internal error: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };
        (
            status,
            Json(ErrorBody {
                error: message,
                error_kind,
            }),
        )
            .into_response()
    }
}
