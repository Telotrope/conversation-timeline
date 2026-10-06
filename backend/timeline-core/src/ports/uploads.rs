//! Tracks the terminal outcome of one upload's processing, and on AWS how
//! many attempts it has taken so far ([`UploadProgress`]). The outcome is
//! written once, when processing finishes; `None` from `get_outcome` means
//! "not done yet". Progress was added on 2026-10-02 so the page can show a
//! retry instead of a silent wait (plan
//! `2026-10-02-upload-processing-failures.md` §3); before that, per the
//! migration plan's §V2a-revision, nothing read an in-progress state, so
//! none was stored. `POST /uploads` still never touches this store. Backed by the
//! `Conversations` DynamoDB table from the migration plan §1.3 -- an
//! upload's own outcome row and the per-conversation summary rows it
//! eventually produces share that table, distinguished by sort key in the
//! concrete adapter, not by a separate table this crate doesn't list.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::{UploadId, UserId};
use crate::conversation_metadata::UploadFacts;
use crate::model::{ConversationId, MessageId};

/// The raw upload's object-store key is a pure function of `(user_id,
/// upload_id)` -- `raw/{user_id}/{upload_id}.json` -- so it is never
/// stored, only recomputed. This is the one place that format string is
/// defined; every caller that needs the key (issuing the presigned PUT URL
/// in `timeline-api::routes::uploads`, the processing pipeline reading the
/// bytes back in `timeline-api::processing`) must call this function
/// rather than repeating the format string, so the two call sites can't
/// drift out of sync with each other.
pub fn raw_object_key(user_id: &UserId, upload_id: UploadId) -> String {
    format!("raw/{user_id}/{upload_id}.json")
}

/// Where one file kept from a conversation is stored (plan
/// `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3, §4):
/// `files/{user_id}/{conversation_id}/{message_id}/{number}`. The file is
/// numbered within its message and its name stays on the message's row, so
/// no name from an upload ever becomes part of a storage key.
pub fn file_object_key(
    user_id: &UserId,
    conversation_id: ConversationId,
    message_id: MessageId,
    number: usize,
) -> String {
    format!("files/{user_id}/{conversation_id}/{message_id}/{number}")
}

/// The inverse of [`raw_object_key`]: the user and upload a raw upload's key
/// names, or `None` for any key [`raw_object_key`] can't have produced (an
/// export, an empty user part, an upload part that isn't a UUID followed by
/// `.json`). Used wherever a stored file has to be traced back to its upload:
/// the local upload route and the S3-triggered processing Lambda.
pub fn parse_raw_object_key(key: &str) -> Option<(UserId, UploadId)> {
    let rest = key.strip_prefix("raw/")?;
    let (user_part, upload_part) = rest.split_once('/')?;
    if user_part.is_empty() {
        return None;
    }
    let upload_id_str = upload_part.strip_suffix(".json")?;
    let upload_id = UploadId(upload_id_str.parse().ok()?);
    Some((UserId(user_part.to_string()), upload_id))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UploadOutcome {
    Ready {
        conversation_ids: Vec<ConversationId>,
    },
    Failed {
        reason: String,
    },
}

/// How far processing has got on AWS before an outcome exists (plan
/// `2026-10-02-upload-processing-failures.md` §3): how many attempts have
/// started, and the last one's error if an attempt failed. Only the
/// S3-triggered path counts attempts; the local server processes in one go.
/// `processing` is how far the running attempt has got (plan
/// `2026-10-06-load-only-what-the-page-shows.md` §8b), written every second.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadProgress {
    pub attempts: usize,
    pub last_error: Option<String>,
    pub processing: Option<ProcessingProgress>,
}

/// How far an attempt has got: bytes of the file read, then conversations
/// written, each of its total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProcessingProgress {
    pub bytes_read: u64,
    pub bytes_total: u64,
    pub conversations_written: usize,
    pub conversations_total: usize,
}

#[async_trait]
pub trait UploadOutcomeStore: Send + Sync {
    /// Written once, when processing finishes -- there is no earlier,
    /// pending write to overwrite, so every adapter implements this as a
    /// plain upsert.
    async fn record_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        outcome: UploadOutcome,
    ) -> Result<(), StoreError>;

    /// `None` means "not finished yet" -- the only status a polling client
    /// needs, without a separate persisted `Pending`/`Processing` value.
    async fn get_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError>;

    /// Counts one more processing attempt and returns its number, from 1.
    /// Kept apart from the outcome, which replaces nothing here.
    async fn record_attempt(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<usize, StoreError>;

    /// Records why the latest attempt failed. Later attempts keep it until
    /// they fail with another error.
    async fn record_attempt_error(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        error: String,
    ) -> Result<(), StoreError>;

    /// Records how far the running attempt has got.
    async fn record_processing_progress(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        progress: ProcessingProgress,
    ) -> Result<(), StoreError>;

    /// `None` until the first attempt starts or reports progress.
    async fn get_progress(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadProgress>, StoreError>;

    /// Records what `POST /uploads` learned about the file, for processing
    /// to read back (plan `2026-10-05-screen-flow.md` §8b). Written once,
    /// before the file's upload address is handed out.
    async fn record_received(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        facts: UploadFacts,
    ) -> Result<(), StoreError>;

    /// `None` when `POST /uploads` never recorded this upload.
    async fn get_received(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadFacts>, StoreError>;
}
