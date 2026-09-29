//! Emptying an in-memory store, for tests that need a known starting state.
//!
//! This is deliberately **not** a storage port. Nothing in the domain asks to
//! have its data erased, and a real S3 bucket or DynamoDB table has no
//! business implementing it -- so it lives here, next to the fakes, rather
//! than in `timeline_core::ports`, and only the in-memory adapters implement
//! it.
//!
//! It exists because tests that share one server cannot assume a neutral
//! starting state. Namespacing each test under its own user id avoids
//! collisions but is not the same thing as a clean slate: it leaves every
//! earlier test's data in the store, where anything not keyed by user still
//! sees it. Resetting says what it means.
//!
//! Reached only through `POST /_dev/reset`, which lives in the `_dev` router
//! and is therefore structurally absent from the Lambda build -- see
//! `timeline_api::app::build_router` and the test that asserts it.

/// An in-memory store that can be emptied.
///
/// `&self` rather than `&mut self` because every implementor guards its own
/// contents with a `Mutex` and is shared as an `Arc`; taking `&mut self`
/// would make it unusable through the `Arc` the app actually holds.
pub trait Resettable: Send + Sync {
    /// Discards everything the store holds. Not undoable, and not something
    /// any production code path should ever call.
    fn reset(&self);
}
