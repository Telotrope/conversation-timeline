//! Reports each AWS call that failed for good -- after the SDK's own retries
//! -- as a `tracing` event naming the operation
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3, critique C16).
//!
//! The SDK has no reliable "gave up" signal of its own: its "halting" debug
//! message also fires on attempts it then retries. The adapters here are
//! where the final error arrives, so they report it. The API crate's
//! `aws_call_counter` counts these events into the current request's log
//! line; with no subscriber listening, reporting costs nothing and changes
//! nothing.

use std::fmt::Display;

/// The `tracing` target the events are sent under.
pub const AWS_FAILURE_TARGET: &str = "timeline_storage::aws_failure";

/// Reports `error` as a failed call to `operation` (`DynamoDB.PutItem`).
pub(crate) fn report(operation: &'static str, error: &dyn Display) {
    tracing::warn!(target: AWS_FAILURE_TARGET, operation, error = %error);
}
