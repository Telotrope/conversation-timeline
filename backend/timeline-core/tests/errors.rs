//! Black-box tests for the storage port error types' `Display`/`source`
//! implementations -- these were never exercised by any other test, since
//! nothing else calls `.to_string()` or inspects `.source()` on them
//! directly.

use std::error::Error;

use timeline_core::ports::errors::{ObjectStoreError, StoreError};

#[derive(Debug)]
struct DummyCause;

impl std::fmt::Display for DummyCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dummy underlying cause")
    }
}
impl Error for DummyCause {}

#[test]
fn store_error_not_found_has_a_message_and_no_source() {
    let err = StoreError::NotFound;
    assert_eq!(err.to_string(), "item not found");
    assert!(err.source().is_none());
}

#[test]
fn store_error_backend_carries_its_message_and_source() {
    let err = StoreError::Backend(Box::new(DummyCause));
    assert!(err.to_string().contains("dummy underlying cause"));
    assert!(err.source().is_some());
}

#[test]
fn object_store_error_not_found_has_a_message_and_no_source() {
    let err = ObjectStoreError::NotFound;
    assert_eq!(err.to_string(), "object not found");
    assert!(err.source().is_none());
}

#[test]
fn object_store_error_backend_carries_its_message_and_source() {
    let err = ObjectStoreError::Backend(Box::new(DummyCause));
    assert!(err.to_string().contains("dummy underlying cause"));
    assert!(err.source().is_some());
}

#[test]
fn a_conflict_and_unwritten_rows_say_what_happened_and_have_no_source() {
    use std::error::Error;
    let conflict = StoreError::Conflict;
    assert_eq!(
        conflict.to_string(),
        "someone else changed this record first"
    );
    assert!(conflict.source().is_none());
    let unwritten = StoreError::Unwritten { left: 3, total: 40 };
    assert_eq!(
        unwritten.to_string(),
        "3 of 40 rows were still unwritten after every retry"
    );
    assert!(unwritten.source().is_none());
}
