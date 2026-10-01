//! Tracks the terminal outcome of one upload's processing. Per the
//! migration plan's §V2a-revision, nothing is written before processing
//! finishes -- there is no persisted `Pending`/`Processing` state, because
//! nothing currently reads one (`POST /uploads` never touches this store at
//! all): a client that needs a "still processing…" indicator can poll
//! `get_outcome` and treat `None` as "not done yet." Backed by the
//! `Conversations` DynamoDB table from the migration plan §1.3 -- an
//! upload's own outcome row and the per-conversation summary rows it
//! eventually produces share that table, distinguished by sort key in the
//! concrete adapter, not by a separate table this crate doesn't list.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::{UploadId, UserId};
use crate::model::ConversationId;

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
}
