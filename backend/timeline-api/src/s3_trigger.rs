//! Handles the S3 "object created" event that starts upload processing on
//! AWS: each record names one uploaded file, which is traced back to its
//! user and upload and handed to [`crate::processing::process_upload`]. Run
//! by the `process_upload` Lambda (`src/bin/process_upload.rs`); see the
//! migration plan's §V2e, E2.
//!
//! What happens on failure, per CLAUDE.md's exception-handling rules:
//! - **A file that isn't a valid export** is not retried: `process_upload`
//!   has already recorded a `Failed` outcome the page will show, and
//!   reading the same bytes again can't help. It's logged, and the record
//!   counts as handled.
//! - **A storage failure**, or a key that can't be traced to an upload, is
//!   returned as an error naming the key, after every other record has been
//!   attempted. Lambda then retries the whole event. That's safe because
//!   every write `process_upload` makes replaces rather than adds (plan C29).
//!   Each attempt is counted first, and a failed attempt's error recorded,
//!   so the page can show the retry (plan
//!   `2026-10-02-upload-processing-failures.md` §3). After the last retry,
//!   the separate `record_failed_upload` function records `Failed` (§2).
//!
//! Every record's run ends with one `processing_run` log line
//! ([`processing_run_line`]; docs/plans/2026-10-02-activity-instrumentation.md
//! §3): the key, user and upload, the outcome, how long it took, what
//! processing read and stored, and the AWS calls it made.
//!
//! [`handle_raw_s3_event`] is what the Lambda runs: it takes the
//! notification as plain JSON, logs it when [`EventLogging::On`] (with the
//! uploader's IP address removed), then reads it and calls
//! [`handle_s3_event`]. Logging happens before reading on purpose: the
//! `S3Event` type drops fields it doesn't declare, and a real notification
//! is logged to capture exactly those (migration plan §V2e, E9; C24).

use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use aws_lambda_events::event::s3::S3Event;
use percent_encoding::percent_decode_str;
use serde_json::{json, Value};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::UserFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{parse_raw_object_key, UploadOutcomeStore};

use crate::aws_settings::{DeliberateFailure, EventLogging};
use crate::processing::{process_upload, ProcessingError};
use crate::request_record::{self, note, note_user, RequestRecord};

/// The stores processing writes through.
#[derive(Clone)]
pub struct ProcessingStores {
    pub object_store: Arc<dyn ObjectStore>,
    pub upload_outcome_store: Arc<dyn UploadOutcomeStore>,
    pub conversation_summary_store: Arc<dyn ConversationSummaryStore>,
    pub user_flag_writer: Arc<dyn UserFlagWriter>,
}

/// How many times AWS runs the processing function for one upload: the
/// first attempt plus the template's `MaximumRetryAttempts` (2). The page
/// shows "attempt 2 of 3" from this; `tests/template_processing_retries.rs`
/// checks the template agrees.
pub const MAX_PROCESSING_ATTEMPTS: usize = 3;

/// One record that couldn't be handled.
#[derive(Debug)]
pub enum RecordError {
    /// The record has no object key at all.
    MissingKey,
    /// The key isn't one `raw_object_key` writes, or can't be decoded.
    UnusableKey { key: String, reason: String },
    /// Processing the upload failed in a way a retry might fix.
    Processing { key: String, error: ProcessingError },
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::MissingKey => write!(f, "an event record has no object key"),
            RecordError::UnusableKey { key, reason } => {
                write!(f, "object key {key:?} is not a raw upload: {reason}")
            }
            RecordError::Processing { key, error } => {
                write!(f, "processing {key:?} failed: {error}")
            }
        }
    }
}

/// Every record that failed, in event order.
#[derive(Debug)]
pub struct TriggerError(pub Vec<RecordError>);

impl fmt::Display for TriggerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(|e| e.to_string()).collect();
        write!(f, "{} record(s) failed: {}", parts.len(), parts.join("; "))
    }
}

impl std::error::Error for TriggerError {}

/// S3 event keys are encoded the way web forms encode values: `+` for a
/// space and `%XX` for other special characters.
pub(crate) fn decode_key(raw: &str) -> Result<String, String> {
    let spaced = raw.replace('+', " ");
    percent_decode_str(&spaced)
        .decode_utf8()
        .map(|k| k.into_owned())
        .map_err(|e| format!("not valid UTF-8 once decoded: {e}"))
}

async fn handle_record(
    key: Option<&str>,
    stores: &ProcessingStores,
    failure: DeliberateFailure,
) -> Result<(), RecordError> {
    let raw = key.ok_or(RecordError::MissingKey)?;
    let key = decode_key(raw).map_err(|reason| RecordError::UnusableKey {
        key: raw.to_string(),
        reason,
    })?;
    let (user_id, upload_id) =
        parse_raw_object_key(&key).ok_or_else(|| RecordError::UnusableKey {
            key: key.clone(),
            reason: "expected raw/<user>/<upload uuid>.json".to_string(),
        })?;
    note_user(&user_id);
    note("upload_id", upload_id.0.to_string());
    // Counted before processing, so the page can say which attempt this is
    // (plan `2026-10-02-upload-processing-failures.md` §3). Failing to count
    // is a storage failure like any other: retried.
    let attempt = stores
        .upload_outcome_store
        .record_attempt(&user_id, upload_id)
        .await
        .map_err(|e| RecordError::Processing {
            key: key.clone(),
            error: ProcessingError::Store(e),
        })?;
    note("attempt", attempt);
    let result = if failure == DeliberateFailure::On {
        Err(ProcessingError::FailingOnPurpose)
    } else {
        process_upload(
            stores.object_store.as_ref(),
            stores.upload_outcome_store.as_ref(),
            stores.conversation_summary_store.as_ref(),
            stores.user_flag_writer.as_ref(),
            &user_id,
            upload_id,
        )
        .await
    };
    match result {
        Ok(()) => Ok(()),
        Err(
            e @ (ProcessingError::RawObjectNotUtf8(_)
            | ProcessingError::Format(_)
            | ProcessingError::ReviewField { .. }),
        ) => {
            // Already recorded as `Failed` for the page; see module doc.
            eprintln!("upload {key:?} is not a usable export (recorded as failed): {e}");
            note("unusable", e.to_string());
            Ok(())
        }
        Err(error) => {
            // Shown on the page as the reason for the retry. If even this
            // write fails, the original error still goes back to Lambda.
            if let Err(e) = stores
                .upload_outcome_store
                .record_attempt_error(&user_id, upload_id, error.to_string())
                .await
            {
                eprintln!("upload {key:?}: couldn't record this attempt's error ({error}): {e}");
            }
            Err(RecordError::Processing { key, error })
        }
    }
}

/// Processes every record in the event; see the module doc for what is
/// retried and what isn't.
pub async fn handle_s3_event(
    event: S3Event,
    stores: &ProcessingStores,
) -> Result<(), TriggerError> {
    handle_s3_event_with(event, stores, DeliberateFailure::Off).await
}

/// [`handle_s3_event`], failing every record on purpose when `failure` is
/// on (after counting the attempt, so the page shows it), for testing the
/// failure path on a real deployment (plan
/// `2026-10-02-upload-processing-failures.md` §2b).
pub async fn handle_s3_event_with(
    event: S3Event,
    stores: &ProcessingStores,
    failure: DeliberateFailure,
) -> Result<(), TriggerError> {
    handle_s3_event_logged(event, stores, failure, &|_line| {}).await
}

/// The `processing_run` line for one record's run: `key` as the event named
/// it, `result` its outcome, `record` what was recorded while it ran.
pub fn processing_run_line(
    key: Option<&str>,
    result: &Result<(), RecordError>,
    millis: u128,
    record: &RequestRecord,
) -> String {
    let mut facts = record.facts.clone();
    let upload_id = facts.remove("upload_id");
    let unusable = facts.remove("unusable");
    let (outcome, error) = match (result, unusable) {
        (Err(e), _) => ("error", Some(Value::String(e.to_string()))),
        (Ok(()), Some(reason)) => ("unusable", Some(reason)),
        (Ok(()), None) => ("ready", None),
    };
    json!({
        "kind": "processing_run",
        "key": key,
        "user": record.user,
        "upload_id": upload_id,
        "outcome": outcome,
        "error": error,
        "ms": millis as u64,
        "facts": Value::Object(facts),
        "aws_calls": record.aws_calls,
        "aws_retries": record.aws_retries,
    })
    .to_string()
}

/// [`handle_s3_event_with`], writing each record's `processing_run` line to
/// `log`.
pub async fn handle_s3_event_logged(
    event: S3Event,
    stores: &ProcessingStores,
    failure: DeliberateFailure,
    log: &dyn Fn(&str),
) -> Result<(), TriggerError> {
    let mut failures = Vec::new();
    for record in &event.records {
        let key = record.s3.object.key.as_deref();
        let started = Instant::now();
        let (result, recorded) =
            request_record::recording(handle_record(key, stores, failure)).await;
        log(&processing_run_line(
            key,
            &result,
            started.elapsed().as_millis(),
            &recorded,
        ));
        if let Err(e) = result {
            eprintln!("{e}");
            failures.push(e);
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(TriggerError(failures))
    }
}

/// The notification with each record's `requestParameters.sourceIPAddress`
/// (the uploader's IP address) replaced by `"REDACTED"`. Nothing else
/// changes; anything missing is left missing.
pub fn redact_s3_event(mut event: Value) -> Value {
    if let Some(records) = event.get_mut("Records").and_then(Value::as_array_mut) {
        for record in records {
            if let Some(ip) = record
                .get_mut("requestParameters")
                .and_then(|p| p.get_mut("sourceIPAddress"))
            {
                *ip = Value::String("REDACTED".to_string());
            }
        }
    }
    event
}

/// The line logged for a notification; `sam logs --filter "s3 event"`
/// finds it.
pub fn s3_event_log_line(event: &Value) -> String {
    format!(
        "s3 event (sourceIPAddress removed): {}",
        redact_s3_event(event.clone())
    )
}

/// Why a raw notification couldn't be handled.
#[derive(Debug)]
pub enum RawEventError {
    /// The JSON isn't an S3 notification `S3Event` can read.
    Unreadable(serde_json::Error),
    /// Read, but one or more records failed; see [`handle_s3_event`].
    Trigger(TriggerError),
}

impl fmt::Display for RawEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RawEventError::Unreadable(e) => write!(f, "not a readable S3 notification: {e}"),
            RawEventError::Trigger(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RawEventError {}

/// What the processing Lambda runs for each notification: logs it through
/// `log` when `logging` is on, then reads and handles it. An unreadable
/// notification is an error, so Lambda retries it and the log says why.
pub async fn handle_raw_s3_event(
    raw: Value,
    stores: &ProcessingStores,
    logging: EventLogging,
    log: impl Fn(&str),
) -> Result<(), RawEventError> {
    handle_raw_s3_event_with(raw, stores, logging, DeliberateFailure::Off, log).await
}

/// [`handle_raw_s3_event`] with the deliberate-failure setting; see
/// [`handle_s3_event_with`].
pub async fn handle_raw_s3_event_with(
    raw: Value,
    stores: &ProcessingStores,
    logging: EventLogging,
    failure: DeliberateFailure,
    log: impl Fn(&str),
) -> Result<(), RawEventError> {
    handle_raw_s3_event_recorded(raw, stores, logging, failure, log, &|_line| {}).await
}

/// What the processing Lambda runs: [`handle_raw_s3_event_with`], also
/// writing each record's `processing_run` line to `run_log`. Kept apart from
/// `log`, which carries only the notification itself and only while
/// [`EventLogging::On`].
pub async fn handle_raw_s3_event_recorded(
    raw: Value,
    stores: &ProcessingStores,
    logging: EventLogging,
    failure: DeliberateFailure,
    log: impl Fn(&str),
    run_log: &dyn Fn(&str),
) -> Result<(), RawEventError> {
    if logging == EventLogging::On {
        log(&s3_event_log_line(&raw));
    }
    let event: S3Event = serde_json::from_value(raw).map_err(RawEventError::Unreadable)?;
    handle_s3_event_logged(event, stores, failure, run_log)
        .await
        .map_err(RawEventError::Trigger)
}
