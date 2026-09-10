//! Error types for the storage ports. Per CLAUDE.md's exception-handling
//! rule, every failure a port can produce is a distinct, named case that
//! surfaces the underlying cause — never a bare `String` or a silently
//! dropped error.

use std::error::Error as StdError;
use std::fmt;

/// A boxed source error, kept generic over the concrete backend (DynamoDB,
/// S3, an in-memory test double, ...) so this crate's ports don't depend on
/// any specific AWS SDK type.
pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

#[derive(Debug)]
pub enum StoreError {
    /// The requested item doesn't exist. Distinct from `Backend` so callers
    /// can tell "not found" (often a normal, expected outcome — e.g. a 404)
    /// apart from "something actually went wrong."
    NotFound,
    /// The backend itself failed (a DynamoDB request error, a serialization
    /// problem, ...); the source is preserved so the caller can log or
    /// inspect what actually happened, not just that something did.
    Backend(BoxError),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "item not found"),
            StoreError::Backend(e) => write!(f, "storage backend error: {e}"),
        }
    }
}

impl StdError for StoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            StoreError::NotFound => None,
            StoreError::Backend(e) => Some(e.as_ref()),
        }
    }
}

#[derive(Debug)]
pub enum ObjectStoreError {
    /// The requested object doesn't exist.
    NotFound,
    /// The backend itself failed (an S3 request error, a presigning
    /// failure, ...), with the original error preserved.
    Backend(BoxError),
}

impl fmt::Display for ObjectStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjectStoreError::NotFound => write!(f, "object not found"),
            ObjectStoreError::Backend(e) => write!(f, "object store backend error: {e}"),
        }
    }
}

impl StdError for ObjectStoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            ObjectStoreError::NotFound => None,
            ObjectStoreError::Backend(e) => Some(e.as_ref()),
        }
    }
}
