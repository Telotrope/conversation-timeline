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

use std::fmt;
use std::sync::Arc;

use aws_lambda_events::event::s3::S3Event;
use percent_encoding::percent_decode_str;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::UserFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{parse_raw_object_key, UploadOutcomeStore};

use crate::processing::{process_upload, ProcessingError};

/// The stores processing writes through.
#[derive(Clone)]
pub struct ProcessingStores {
    pub object_store: Arc<dyn ObjectStore>,
    pub upload_outcome_store: Arc<dyn UploadOutcomeStore>,
    pub conversation_summary_store: Arc<dyn ConversationSummaryStore>,
    pub user_flag_writer: Arc<dyn UserFlagWriter>,
}

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
fn decode_key(raw: &str) -> Result<String, String> {
    let spaced = raw.replace('+', " ");
    percent_decode_str(&spaced)
        .decode_utf8()
        .map(|k| k.into_owned())
        .map_err(|e| format!("not valid UTF-8 once decoded: {e}"))
}

async fn handle_record(key: Option<&str>, stores: &ProcessingStores) -> Result<(), RecordError> {
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
    let result = process_upload(
        stores.object_store.as_ref(),
        stores.upload_outcome_store.as_ref(),
        stores.conversation_summary_store.as_ref(),
        stores.user_flag_writer.as_ref(),
        &user_id,
        upload_id,
    )
    .await;
    match result {
        Ok(()) => Ok(()),
        Err(
            e @ (ProcessingError::RawObjectNotUtf8(_)
            | ProcessingError::Format(_)
            | ProcessingError::ReviewField { .. }),
        ) => {
            // Already recorded as `Failed` for the page; see module doc.
            eprintln!("upload {key:?} is not a usable export (recorded as failed): {e}");
            Ok(())
        }
        Err(error) => Err(RecordError::Processing { key, error }),
    }
}

/// Processes every record in the event; see the module doc for what is
/// retried and what isn't.
pub async fn handle_s3_event(
    event: S3Event,
    stores: &ProcessingStores,
) -> Result<(), TriggerError> {
    let mut failures = Vec::new();
    for record in &event.records {
        if let Err(e) = handle_record(record.s3.object.key.as_deref(), stores).await {
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
