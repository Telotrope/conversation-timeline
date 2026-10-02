//! Records `Failed` for an upload whose processing failed every attempt
//! (plan `2026-10-02-upload-processing-failures.md` §2). Run by the
//! `record_failed_upload` Lambda (`src/bin/record_failed_upload.rs`), which
//! AWS calls as the processing function's "on failure" destination once its
//! last retry has failed, for any reason: our error, the time limit, or
//! running out of memory. Without it, an upload that never succeeds stays
//! "processing" forever, because the processing function deliberately
//! doesn't record storage errors as failures (`s3_trigger`'s module doc).
//!
//! The input is AWS's invocation record: the original S3 notification as
//! `requestPayload`, and the function's error as `responsePayload`. The
//! test sample of it is written from AWS's documentation, not captured
//! (plan C5).

use std::fmt;

use aws_lambda_events::event::s3::S3Event;
use serde_json::Value;
use timeline_core::ports::uploads::{parse_raw_object_key, UploadOutcome, UploadOutcomeStore};

use crate::s3_trigger::{decode_key, MAX_PROCESSING_ATTEMPTS};

/// Why an invocation record couldn't be turned into `Failed` outcomes.
#[derive(Debug)]
pub enum FailedUploadError {
    /// There's no `requestPayload`: not an invocation record.
    NotAnInvocationRecord,
    /// `requestPayload` isn't an S3 notification.
    UnreadableNotification(serde_json::Error),
    /// A record's object key isn't a raw upload's.
    UnusableKey { key: String, reason: String },
    /// Writing the outcome failed.
    Store {
        key: String,
        error: timeline_core::ports::errors::StoreError,
    },
}

impl fmt::Display for FailedUploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailedUploadError::NotAnInvocationRecord => {
                write!(
                    f,
                    "not a Lambda invocation record: there is no requestPayload"
                )
            }
            FailedUploadError::UnreadableNotification(e) => {
                write!(f, "requestPayload is not a readable S3 notification: {e}")
            }
            FailedUploadError::UnusableKey { key, reason } => {
                write!(f, "object key {key:?} is not a raw upload: {reason}")
            }
            FailedUploadError::Store { key, error } => {
                write!(f, "recording {key:?} as failed: {error}")
            }
        }
    }
}

impl std::error::Error for FailedUploadError {}

/// The reason the page shows: how many attempts, and the last error AWS
/// reported. Falls back to the error type, then to AWS's condition, when a
/// failure carries no message (a time limit or a crash can't say much).
fn reason(record: &Value) -> String {
    let attempts = record["requestContext"]["approximateInvokeCount"]
        .as_u64()
        .map_or(MAX_PROCESSING_ATTEMPTS, |n| n as usize);
    let error = [
        &record["responsePayload"]["errorMessage"],
        &record["responsePayload"]["errorType"],
        &record["requestContext"]["condition"],
    ]
    .into_iter()
    .find_map(Value::as_str)
    .unwrap_or("no error message was given");
    format!("the server couldn't process the file after {attempts} attempts: {error}")
}

/// Records `Failed` for every upload the record's notification names. Keys
/// are all attempted; the first error is returned, so AWS logs it.
pub async fn handle_failed_invocation(
    record: Value,
    store: &dyn UploadOutcomeStore,
) -> Result<(), FailedUploadError> {
    let payload = record
        .get("requestPayload")
        .ok_or(FailedUploadError::NotAnInvocationRecord)?;
    let event: S3Event = serde_json::from_value(payload.clone())
        .map_err(FailedUploadError::UnreadableNotification)?;
    let reason = reason(&record);
    let mut first_error = None;
    for s3_record in &event.records {
        let raw = s3_record.s3.object.key.as_deref().unwrap_or("");
        let result = async {
            let key = decode_key(raw).map_err(|reason| FailedUploadError::UnusableKey {
                key: raw.to_string(),
                reason,
            })?;
            let (user_id, upload_id) =
                parse_raw_object_key(&key).ok_or_else(|| FailedUploadError::UnusableKey {
                    key: key.clone(),
                    reason: "expected raw/<user>/<upload uuid>.json".to_string(),
                })?;
            store
                .record_outcome(
                    &user_id,
                    upload_id,
                    UploadOutcome::Failed {
                        reason: reason.clone(),
                    },
                )
                .await
                .map_err(|error| FailedUploadError::Store { key, error })
        }
        .await;
        if let Err(e) = result {
            eprintln!("{e}");
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(()), Err)
}
