//! Real DynamoDB-backed adapters. Everything lives in the one
//! `Conversations` table, told apart by sort-key prefix: `CONV#` records,
//! `SESS#` sessions, `MSG#` message rows, `USER` the user's record,
//! `ANALYSIS#` saved analyses, and the upload rows. Tested against Amazon's
//! DynamoDB Local in `tests/dynamo_*.rs` (the shared storage contracts, plus
//! rows our code didn't write and a missing table).

mod attributes;
mod batches;
pub mod conversations_table;
pub mod message_rows;
mod query;
pub mod sessions_table;
pub mod user_record_rows;

use timeline_core::ports::errors::StoreError;

use crate::aws_failure::report;

/// Maps a failed call to `operation` to a backend error, reporting it for
/// the request's log line (`crate::aws_failure`).
pub(crate) fn backend_error<E: std::error::Error + Send + Sync + 'static>(
    operation: &'static str,
) -> impl FnOnce(E) -> StoreError {
    move |e| {
        report(operation, &e);
        StoreError::Backend(Box::new(e))
    }
}
