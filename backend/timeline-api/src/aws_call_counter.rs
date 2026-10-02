//! Counts each request's calls to AWS, for its log line
//! (docs/plans/2026-10-02-activity-instrumentation.md §3, critique C5).
//!
//! The AWS SDK opens a `tracing` span for every operation it performs,
//! named after the service and operation (`DynamoDB.PutItem`, `S3.GetObject`;
//! read in aws-sdk-dynamodb 1.124's generated operation code), and one span
//! per attempt, `try_attempt`, with an `attempt` number starting at 1
//! (aws-smithy-runtime 1.14's orchestrator). [`AwsCallCounter`] watches only
//! those two kinds of span and adds them to the current
//! [`crate::request_record`]: an operation span is one call, an attempt
//! numbered 2 or more is one retry. A call that failed for good is reported
//! by our storage adapters as an event (`timeline_storage::aws_failure`),
//! which it counts as one failure. Nothing is printed by it.
//!
//! Every other span and event is declined when its call site is first
//! registered, so the rest of the program's `tracing` output costs one
//! cached check and is not collected.

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::subscriber::Interest;
use tracing::Event;
use tracing::{Metadata, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Layer;

use timeline_storage::aws_failure::AWS_FAILURE_TARGET;

use crate::request_record::{count_aws_call, count_aws_failure, count_aws_retry};

const RETRY_SPAN: &str = "try_attempt";

/// The `tracing` layer described in the module doc.
pub struct AwsCallCounter;

/// An SDK operation span: its target is the SDK crate (`aws_sdk_dynamodb`)
/// and its name is `Service.Operation`.
fn is_operation_span(metadata: &Metadata<'_>) -> bool {
    metadata.is_span() && metadata.target().starts_with("aws_sdk_") && metadata.name().contains('.')
}

fn is_retry_span(metadata: &Metadata<'_>) -> bool {
    metadata.is_span()
        && metadata.name() == RETRY_SPAN
        && metadata.target().starts_with("aws_smithy_runtime")
}

fn is_failure_event(metadata: &Metadata<'_>) -> bool {
    metadata.is_event() && metadata.target() == AWS_FAILURE_TARGET
}

fn is_watched(metadata: &Metadata<'_>) -> bool {
    is_operation_span(metadata) || is_retry_span(metadata) || is_failure_event(metadata)
}

/// A failure event's `operation` and `error` fields.
#[derive(Default)]
struct Failure {
    operation: String,
    error: String,
}

impl Visit for Failure {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "operation" {
            self.operation = value.to_string();
        }
    }

    // `error` arrives here (it is sent with `%`, as text through Display);
    // anything else is ignored.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "error" {
            self.error = format!("{value:?}");
        }
    }
}

/// Reads a span's `attempt` field.
struct AttemptNumber(Option<u64>);

impl Visit for AttemptNumber {
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "attempt" {
            self.0 = Some(value);
        }
    }

    // Required by `Visit`. The only span read here, `try_attempt`, has one
    // field, a number, which arrives through `record_u64`; so this is never
    // called today. Kept as a backstop: a field of another type is ignored.
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

/// A span's `attempt` field, 0 when it has none.
fn attempt_number(attrs: &Attributes<'_>) -> u64 {
    let mut attempt = AttemptNumber(None);
    attrs.record(&mut attempt);
    attempt.0.unwrap_or(0)
}

impl<S: Subscriber> Layer<S> for AwsCallCounter {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if is_watched(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
        let metadata = attrs.metadata();
        // Checked again here, not assumed from `register_callsite`: where
        // several subscribers share a call site (tests), other spans can
        // arrive too.
        if is_operation_span(metadata) {
            count_aws_call(metadata.name());
        } else if is_retry_span(metadata) && attempt_number(attrs) >= 2 {
            count_aws_retry();
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if is_failure_event(event.metadata()) {
            let mut failure = Failure::default();
            event.record(&mut failure);
            count_aws_failure(&failure.operation, &failure.error);
        }
    }
}

/// The subscriber the Lambda binaries install as the process-wide default.
pub fn counting_subscriber() -> impl Subscriber + Send + Sync {
    tracing_subscriber::registry().with(AwsCallCounter)
}

/// Installs [`counting_subscriber`] for the whole process. Called once at
/// the start of each Lambda binary that makes AWS calls; a second install
/// would fail, which `expect` reports at start-up.
pub fn install() {
    tracing::subscriber::set_global_default(counting_subscriber())
        .expect("the AWS call counter is installed once, at start-up");
}
