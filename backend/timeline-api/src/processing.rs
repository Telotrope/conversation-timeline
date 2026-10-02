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
//!
//! Per the migration plan's §V2a-revision, there is no pre-existing upload
//! record to look up: the raw object's key is recomputed from
//! `(user_id, upload_id)` via [`timeline_core::ports::uploads::raw_object_key`],
//! and a missing object surfaces as `ProcessingError::ObjectStore`'s
//! `NotFound` case rather than a separate "upload not found" concept.

use std::fmt;

use timeline_core::flags::anger::detect_angry;
use timeline_core::flags::caps::has_emphasis_caps;
use timeline_core::flags::criticism::detect_critical;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::message_flags::{FlagOverrides, FlagSet, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome, UploadOutcomeStore};
use timeline_core::{unwrap_uploaded_json, FormatError};

#[derive(Debug)]
pub enum ProcessingError {
    RawObjectNotUtf8(std::string::FromUtf8Error),
    Format(FormatError),
    Store(StoreError),
    ObjectStore(ObjectStoreError),
    /// A message's `_claude_timeline_user` field is not a review this
    /// project wrote: not an object of optional caps/critical/angry booleans.
    ReviewField { message_id: MessageId, error: serde_json::Error },
    /// Storing one of the reviews embedded in the file failed. Numbered from
    /// 1 in the order they're saved, so a log line says how far it got
    /// (plan `2026-10-02-upload-processing-failures.md` §1a).
    SavingReview {
        number: usize,
        total: usize,
        conversation_id: ConversationId,
        message_id: MessageId,
        source: StoreError,
    },
    /// Storing one conversation's summary failed; numbered like `SavingReview`.
    SavingSummary {
        number: usize,
        total: usize,
        conversation_id: ConversationId,
        source: StoreError,
    },
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessingError::RawObjectNotUtf8(e) => {
                write!(f, "uploaded file was not valid UTF-8: {e}")
            }
            ProcessingError::Format(e) => write!(f, "{e}"),
            ProcessingError::Store(e) => write!(f, "{e}"),
            ProcessingError::ObjectStore(e) => write!(f, "{e}"),
            ProcessingError::ReviewField { message_id, error } => {
                write!(f, "message {message_id} has an unreadable _claude_timeline_user review: {error}")
            }
            ProcessingError::SavingReview { number, total, conversation_id, message_id, source } => {
                write!(f, "saving review {number} of {total} (conversation {conversation_id}, message {message_id}): {source}")
            }
            ProcessingError::SavingSummary { number, total, conversation_id, source } => {
                write!(f, "saving conversation summary {number} of {total} (conversation {conversation_id}): {source}")
            }
        }
    }
}

impl std::error::Error for ProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProcessingError::RawObjectNotUtf8(e) => Some(e),
            ProcessingError::Format(e) => Some(e),
            ProcessingError::Store(e) => Some(e),
            ProcessingError::ObjectStore(e) => Some(e),
            ProcessingError::ReviewField { error, .. } => Some(error),
            ProcessingError::SavingReview { source, .. } => Some(source),
            ProcessingError::SavingSummary { source, .. } => Some(source),
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

/// The non-generative pass: dictionary-checked ALL-CAPS emphasis plus
/// keyword/sentiment criticism and anger. Shared with `routes::detect`,
/// which is the only thing that runs it now.
pub fn heuristic_flags(text: &str) -> FlagSet {
    FlagSet {
        caps: has_emphasis_caps(text),
        critical: detect_critical(text),
        angry: detect_angry(text),
    }
}

/// Reads the raw upload, parses and dedups it, stores any reviews embedded in
/// it, writes one summary per conversation, then records a `Ready` outcome. On a parse failure a
/// `Failed` outcome is recorded with the reason before the error is returned
/// to the caller — callers should never need to separately record failure
/// themselves.
///
/// **This does not compute flags.** Detection is a separate, user-triggered
/// pass (`POST /detect`, see `crate::routes::detect`): it is work
/// proportional to the number of speech acts in the export, and running it
/// here made every upload pay for it whether or not anyone had asked for
/// automatic tags. A freshly uploaded export therefore has no automatic
/// flags until detection is requested, which is the intended behavior and
/// not a missing write.
pub async fn process_upload(
    object_store: &dyn ObjectStore,
    upload_outcome_store: &dyn UploadOutcomeStore,
    conversation_summary_store: &dyn ConversationSummaryStore,
    user_flag_writer: &dyn UserFlagWriter,
    user_id: &UserId,
    upload_id: UploadId,
) -> Result<(), ProcessingError> {
    let key = raw_object_key(user_id, upload_id);
    let raw_bytes = object_store.get(&key).await?;
    let raw_text = match String::from_utf8(raw_bytes) {
        Ok(t) => t,
        Err(e) => {
            let reason = format!("uploaded file was not valid UTF-8: {e}");
            upload_outcome_store
                .record_outcome(user_id, upload_id, UploadOutcome::Failed { reason })
                .await?;
            return Err(ProcessingError::RawObjectNotUtf8(e));
        }
    };

    let parsed = match unwrap_uploaded_json(&raw_text) {
        Ok(p) => p,
        Err(e) => {
            let reason = e.to_string();
            upload_outcome_store
                .record_outcome(user_id, upload_id, UploadOutcome::Failed { reason })
                .await?;
            return Err(ProcessingError::Format(e));
        }
    };

    // Reviews you made earlier travel inside the file, in each message's
    // `_claude_timeline_user` field. Stored here exactly as a tick in the page
    // stores them, so the server's record of your reviews is the only one:
    // detection, which creates records for every message, can't blank them.
    // A field that isn't a review fails the upload, before anything is stored.
    let mut reviews = Vec::new();
    for conversation in &parsed.conversations {
        for message in &conversation.chat_messages {
            if message.sender != Sender::Human {
                continue;
            }
            let Some(value) = message.extra.get("_claude_timeline_user") else {
                continue;
            };
            let review: FlagOverrides = match serde_json::from_value(value.clone()) {
                Ok(r) => r,
                Err(error) => {
                    let err = ProcessingError::ReviewField { message_id: message.uuid, error };
                    upload_outcome_store
                        .record_outcome(user_id, upload_id, UploadOutcome::Failed { reason: err.to_string() })
                        .await?;
                    return Err(err);
                }
            };
            if review != FlagOverrides::default() {
                reviews.push((conversation.uuid, message.uuid, review));
            }
        }
    }
    let total = reviews.len();
    for (index, (conversation_id, message_id, review)) in reviews.into_iter().enumerate() {
        user_flag_writer
            .set_user_flags(user_id, conversation_id, message_id, review)
            .await
            .map_err(|source| ProcessingError::SavingReview {
                number: index + 1,
                total,
                conversation_id,
                message_id,
                source,
            })?;
    }

    let total = parsed.conversations.len();
    let mut conversation_ids = Vec::with_capacity(total);
    for (index, conversation) in parsed.conversations.iter().enumerate() {
        let summary = ConversationSummary {
            conversation_id: conversation.uuid,
            upload_id,
            name: conversation.name.clone(),
            message_count: conversation.chat_messages.len(),
        };
        conversation_summary_store
            .put(user_id, summary)
            .await
            .map_err(|source| ProcessingError::SavingSummary {
                number: index + 1,
                total,
                conversation_id: conversation.uuid,
                source,
            })?;
        conversation_ids.push(conversation.uuid);
    }

    upload_outcome_store
        .record_outcome(
            user_id,
            upload_id,
            UploadOutcome::Ready { conversation_ids },
        )
        .await?;
    Ok(())
}
