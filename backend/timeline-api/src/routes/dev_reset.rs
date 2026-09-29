//! `POST /_dev/reset` -- empties every in-memory store.
//!
//! Exists so an integration test can start from a known state instead of
//! inheriting whatever earlier tests left behind. Giving each test its own
//! user id avoids collisions, but that is not the same as a clean slate: the
//! data is still there, and anything not keyed by user still sees it.
//!
//! Local-dev only, in two senses. It is in the `_dev` router, which
//! `main.rs` merges in only when not running under Lambda, and the stores it
//! can empty are the in-memory fakes -- `Resettable` is implemented by those
//! and nothing else, so there is no reachable path by which this could clear
//! a real S3 bucket or DynamoDB table. `tests/lambda_router.rs` asserts the
//! first of those two claims rather than assuming it.
//!
//! Unauthenticated, like `POST /_dev/login` beside it: requiring a token to
//! reset a throwaway local store would only mean every test logs in twice.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use timeline_storage::memory::resettable::Resettable;

pub async fn reset(State(stores): State<Arc<Vec<Arc<dyn Resettable>>>>) -> StatusCode {
    for store in stores.iter() {
        store.reset();
    }
    StatusCode::NO_CONTENT
}
