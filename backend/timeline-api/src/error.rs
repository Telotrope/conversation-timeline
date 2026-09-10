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

#[derive(Serialize)]
struct ErrorBody {
    error: String,
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
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
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
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
