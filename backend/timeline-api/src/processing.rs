//! Turns an uploaded `conversations.json` into stored conversation
//! summaries and auto-detected flags. One orchestration function, generic
//! over the port traits so it's testable against the in-memory fakes and
//! reusable by both callers the migration plan's §V2a describes: a real
//! S3-triggered Lambda in production (`src/bin/process_upload.rs`) and the
//! local-dev `_dev/local-storage` endpoint
//! (`src/routes/dev_local_storage.rs`).
//!
//! No new domain logic lives here — `unwrap_uploaded_json`, the dedup pass,
//! and the three per-message heuristics were already built and
//! 100%-covered by public-API tests in V1 (`timeline-core`). This module
//! only composes them with the storage ports. Flags are only computed for
//! human-sent messages, matching the original `parseUploadedConversations`
//! at [timeline.html:64963-65020](../../../timeline.html#L64963) (only
//! pushed to `humanMessages` when `m.sender === 'human'`) — Claude's own
//! replies were never a target for these heuristics.

use std::fmt;

use timeline_core::flags::anger::detect_angry;
use timeline_core::flags::caps::has_emphasis_caps;
use timeline_core::flags::criticism::detect_critical;
use timeline_core::ports::conversations::{ConversationStore, ConversationSummary};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::message_flags::{AutoFlagWriter, FlagSet};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadStore;
use timeline_core::{extract_text, unwrap_uploaded_json, FormatError, Sender};

#[derive(Debug)]
pub enum ProcessingError {
    UploadNotFound,
    RawObjectNotUtf8(std::string::FromUtf8Error),
    Format(FormatError),
    Store(StoreError),
    ObjectStore(ObjectStoreError),
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessingError::UploadNotFound => write!(f, "upload not found"),
            ProcessingError::RawObjectNotUtf8(e) => {
                write!(f, "uploaded file was not valid UTF-8: {e}")
            }
            ProcessingError::Format(e) => write!(f, "{e}"),
            ProcessingError::Store(e) => write!(f, "{e}"),
            ProcessingError::ObjectStore(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProcessingError::UploadNotFound => None,
            ProcessingError::RawObjectNotUtf8(e) => Some(e),
            ProcessingError::Format(e) => Some(e),
            ProcessingError::Store(e) => Some(e),
            ProcessingError::ObjectStore(e) => Some(e),
        }
    }
}

impl From<StoreError> for ProcessingError {
    fn from(e: StoreError) -> Self {
        ProcessingError::Store(e)
    }
}

impl From<ObjectStoreError> for ProcessingError {
    fn from(e: ObjectStoreError) -> Self {
        ProcessingError::ObjectStore(e)
    }
}

fn heuristic_flags(text: &str) -> FlagSet {
    FlagSet {
        caps: has_emphasis_caps(text),
        critical: detect_critical(text),
        angry: detect_angry(text),
    }
}

/// Reads the raw upload, parses and dedups it, writes one summary per
/// conversation and one auto-flag set per human message, then marks the
/// upload `Ready`. On a parse failure the upload is marked `Failed` with
/// the reason before the error is returned to the caller — callers should
/// never need to separately call `mark_failed` themselves.
pub async fn process_upload(
    object_store: &dyn ObjectStore,
    upload_store: &dyn UploadStore,
    conversation_store: &dyn ConversationStore,
    auto_flag_writer: &dyn AutoFlagWriter,
    user_id: &UserId,
    upload_id: UploadId,
) -> Result<(), ProcessingError> {
    let record = upload_store
        .get(user_id, upload_id)
        .await?
        .ok_or(ProcessingError::UploadNotFound)?;

    upload_store.mark_processing(user_id, upload_id).await?;

    let raw_bytes = object_store.get(&record.raw_object_key).await?;
    let raw_text = match String::from_utf8(raw_bytes) {
        Ok(t) => t,
        Err(e) => {
            let reason = format!("uploaded file was not valid UTF-8: {e}");
            upload_store
                .mark_failed(user_id, upload_id, reason)
                .await?;
            return Err(ProcessingError::RawObjectNotUtf8(e));
        }
    };

    let parsed = match unwrap_uploaded_json(&raw_text) {
        Ok(p) => p,
        Err(e) => {
            let reason = e.to_string();
            upload_store
                .mark_failed(user_id, upload_id, reason)
                .await?;
            return Err(ProcessingError::Format(e));
        }
    };

    let mut conversation_ids = Vec::with_capacity(parsed.conversations.len());
    for conversation in &parsed.conversations {
        let summary = ConversationSummary {
            conversation_id: conversation.uuid,
            upload_id,
            name: conversation.name.clone(),
            message_count: conversation.chat_messages.len(),
        };
        conversation_store.create(user_id, summary).await?;
        conversation_ids.push(conversation.uuid);

        for message in &conversation.chat_messages {
            if message.sender != Sender::Human {
                continue;
            }
            let flags = heuristic_flags(&extract_text(message));
            auto_flag_writer
                .set_auto_flags(user_id, conversation.uuid, message.uuid, flags)
                .await?;
        }
    }

    upload_store
        .mark_ready(user_id, upload_id, conversation_ids)
        .await?;
    Ok(())
}
