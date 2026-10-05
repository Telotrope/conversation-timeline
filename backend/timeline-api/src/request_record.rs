//! What one request (or one processing run) did, collected while it runs and
//! written as a single log line when it ends
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3).
//!
//! The record lives in a task-local slot for exactly the duration of
//! [`recording`]. Code anywhere underneath -- a route handler, the login
//! check, `processing::process_upload`, the AWS-call counter in
//! `crate::aws_call_counter` -- adds to it with [`note`], [`note_user`] and
//! [`count_aws_call`] without having it passed down. Outside [`recording`]
//! those calls do nothing, which is what the local tests that call handlers
//! directly rely on: recording is an addition to a request, never a
//! condition for it to work.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;

use serde_json::{Map, Value};
use timeline_core::ports::ids::UserId;

/// Everything collected for one request or run.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RequestRecord {
    /// The signed-in user, once the login check has passed.
    pub user: Option<String>,
    /// Facts specific to the route or run, e.g. a detection page's offset.
    pub facts: Map<String, Value>,
    /// How many calls were made to each AWS operation, by the SDK's own
    /// name for it (`DynamoDB.PutItem`). Sorted, so lines compare equal.
    pub aws_calls: BTreeMap<String, u64>,
    /// How many times the SDK retried a call after a failed attempt.
    pub aws_retries: u64,
    /// Calls that still failed once the SDK stopped retrying, by operation.
    pub aws_failures: BTreeMap<String, u64>,
    /// The first [`MAX_KEPT_ERRORS`] of those failures' messages, each cut
    /// to [`MAX_ERROR_CHARS`] characters.
    pub aws_errors: Vec<String>,
}

pub const MAX_KEPT_ERRORS: usize = 3;
pub const MAX_ERROR_CHARS: usize = 200;

tokio::task_local! {
    static CURRENT: RefCell<RequestRecord>;
}

/// Runs `future` with a fresh, empty record, and returns its output together
/// with everything recorded while it ran.
pub async fn recording<F: Future>(future: F) -> (F::Output, RequestRecord) {
    CURRENT
        .scope(RefCell::new(RequestRecord::default()), async move {
            let output = future.await;
            let record = CURRENT.with(|r| r.take());
            (output, record)
        })
        .await
}

/// Adds a fact to the current record. Does nothing outside [`recording`].
pub fn note(key: &'static str, value: impl Into<Value>) {
    let value = value.into();
    with_current(|r| {
        r.facts.insert(key.to_string(), value);
    });
}

/// Records who made the request. Does nothing outside [`recording`].
pub fn note_user(user_id: &UserId) {
    with_current(|r| r.user = Some(user_id.to_string()));
}

/// Counts one call to an AWS operation. Does nothing outside [`recording`].
pub fn count_aws_call(operation: &str) {
    with_current(|r| *r.aws_calls.entry(operation.to_string()).or_insert(0) += 1);
}

/// Counts one retried AWS call attempt. Does nothing outside [`recording`].
pub fn count_aws_retry() {
    with_current(|r| r.aws_retries += 1);
}

/// Counts one AWS call that failed for good, keeping its message if fewer
/// than [`MAX_KEPT_ERRORS`] are kept. Does nothing outside [`recording`].
pub fn count_aws_failure(operation: &str, error: &str) {
    with_current(|r| {
        *r.aws_failures.entry(operation.to_string()).or_insert(0) += 1;
        if r.aws_errors.len() < MAX_KEPT_ERRORS {
            r.aws_errors.push(format!(
                "{operation}: {}",
                error.chars().take(MAX_ERROR_CHARS).collect::<String>()
            ));
        }
    });
}

/// Runs `change` on the current record if there is one. `try_with` fails
/// only when no record is in scope, which is the documented "do nothing"
/// case, not an error.
fn with_current(change: impl FnOnce(&mut RequestRecord)) {
    let _not_recording = CURRENT.try_with(|r| change(&mut r.borrow_mut()));
}
