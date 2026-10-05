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

use timeline_core::conversation_metadata::{guess_summary, message_span, MetadataOrigin};
use timeline_core::flags::anger::detect_angry;
use timeline_core::flags::caps::has_emphasis_caps;
use timeline_core::flags::criticism::detect_critical;
use timeline_core::model::{ChatMessage, Conversation, ConversationId, MessageId, Sender};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::message_flags::{FlagOverrides, FlagSet, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{
    addition_object_key, raw_object_key, UploadOutcome, UploadOutcomeStore,
};
use timeline_core::{unwrap_uploaded_json, FormatError};

use crate::request_record::note;

#[derive(Debug)]
pub enum ProcessingError {
    /// `POST /uploads` never recorded this upload, so its file name, upload
    /// time and human name are unknown. Retrying can't help.
    NoUploadRecord,
    RawObjectNotUtf8(std::string::FromUtf8Error),
    Format(FormatError),
    Store(StoreError),
    ObjectStore(ObjectStoreError),
    /// A message's `_claude_timeline_user` field is not a review this
    /// project wrote: not an object of optional caps/critical/angry booleans.
    ReviewField {
        message_id: MessageId,
        error: serde_json::Error,
    },
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
    /// The `FailProcessing` test setting is on: the attempt fails before
    /// reading the file (plan `2026-10-02-upload-processing-failures.md`
    /// §2b). Retried like a storage failure.
    FailingOnPurpose,
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
            ProcessingError::NoUploadRecord => {
                write!(
                    f,
                    "there is no record of this upload being started (POST /uploads)"
                )
            }
            ProcessingError::RawObjectNotUtf8(e) => {
                write!(f, "uploaded file was not valid UTF-8: {e}")
            }
            ProcessingError::Format(e) => write!(f, "{e}"),
            ProcessingError::Store(e) => write!(f, "{e}"),
            ProcessingError::ObjectStore(e) => write!(f, "{e}"),
            ProcessingError::ReviewField { message_id, error } => {
                write!(
                    f,
                    "message {message_id} has an unreadable _claude_timeline_user review: {error}"
                )
            }
            ProcessingError::FailingOnPurpose => {
                write!(f, "failing on purpose (FailProcessing is on)")
            }
            ProcessingError::SavingReview {
                number,
                total,
                conversation_id,
                message_id,
                source,
            } => {
                write!(f, "saving review {number} of {total} (conversation {conversation_id}, message {message_id}): {source}")
            }
            ProcessingError::SavingSummary {
                number,
                total,
                conversation_id,
                source,
            } => {
                write!(f, "saving conversation summary {number} of {total} (conversation {conversation_id}): {source}")
            }
        }
    }
}

impl std::error::Error for ProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProcessingError::NoUploadRecord => None,
            ProcessingError::RawObjectNotUtf8(e) => Some(e),
            ProcessingError::Format(e) => Some(e),
            ProcessingError::Store(e) => Some(e),
            ProcessingError::ObjectStore(e) => Some(e),
            ProcessingError::ReviewField { error, .. } => Some(error),
            ProcessingError::FailingOnPurpose => None,
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
    // For the run's log line (crate::s3_trigger); nothing outside a
    // recording sees these (crate::request_record).
    note("bytes", raw_bytes.len());
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
                    let err = ProcessingError::ReviewField {
                        message_id: message.uuid,
                        error,
                    };
                    upload_outcome_store
                        .record_outcome(
                            user_id,
                            upload_id,
                            UploadOutcome::Failed {
                                reason: err.to_string(),
                            },
                        )
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
    note("reviews", total);
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

    // Read only now, after the file has been read and checked, so a file
    // that hasn't arrived, or isn't an export, is reported as that rather
    // than as a missing record.
    let Some(facts) = upload_outcome_store
        .get_received(user_id, upload_id)
        .await?
    else {
        upload_outcome_store
            .record_outcome(
                user_id,
                upload_id,
                UploadOutcome::Failed {
                    reason: ProcessingError::NoUploadRecord.to_string(),
                },
            )
            .await?;
        return Err(ProcessingError::NoUploadRecord);
    };
    let total = parsed.conversations.len();
    note("conversations", total);
    let mut conversation_ids = Vec::with_capacity(total);
    let mut report = MergeReport::default();
    for (index, conversation) in parsed.conversations.iter().enumerate() {
        let saving = |source| ProcessingError::SavingSummary {
            number: index + 1,
            total,
            conversation_id: conversation.uuid,
            source,
        };
        let summary = match conversation_summary_store
            .get(user_id, conversation.uuid)
            .await
            .map_err(saving)?
        {
            None => {
                report.new += 1;
                Some(guess_summary(conversation, upload_id, &facts))
            }
            Some(stored) => {
                report.already_present += 1;
                merge_into(
                    object_store,
                    user_id,
                    upload_id,
                    conversation,
                    stored,
                    &mut report,
                )
                .await?
            }
        };
        if let Some(summary) = summary {
            conversation_summary_store
                .put(user_id, summary)
                .await
                .map_err(saving)?;
        }
        conversation_ids.push(conversation.uuid);
    }
    report.note();

    upload_outcome_store
        .record_outcome(
            user_id,
            upload_id,
            UploadOutcome::Ready { conversation_ids },
        )
        .await?;
    Ok(())
}

/// What processing a file did to conversations already stored (plan
/// `2026-10-05-screen-flow.md` §8b-2), for the run's log line.
#[derive(Default)]
struct MergeReport {
    new: usize,
    already_present: usize,
    gained_messages: usize,
    earliest_added: Option<chrono::DateTime<chrono::Utc>>,
    latest_added: Option<chrono::DateTime<chrono::Utc>>,
    /// Conversations that, after the merge, hold fewer messages than this
    /// file's copy of them: a sign the timeframe rule missed messages inside
    /// the stored range (plan C18).
    fewer_than_file: usize,
}

impl MergeReport {
    /// Logged only when the file met conversations already stored; for a
    /// file of only new conversations every count would just repeat
    /// `conversations`.
    fn note(&self) {
        if self.already_present == 0 {
            return;
        }
        note("conversations_new", self.new);
        note("conversations_already_present", self.already_present);
        note("conversations_gained_messages", self.gained_messages);
        if let (Some(earliest), Some(latest)) = (self.earliest_added, self.latest_added) {
            note("earliest_added_message", earliest.to_rfc3339());
            note("latest_added_message", latest.to_rfc3339());
        }
        if self.fewer_than_file > 0 {
            note("conversations_fewer_than_file", self.fewer_than_file);
        }
    }
}

/// A conversation this user already has, met again in a later file. Only
/// the messages timed outside the stored conversation's message range are
/// added (the user's rule, plan §8b-2 and Q18); stored messages are never
/// compared with the file's. The added messages are stored on their own,
/// so nothing has to re-read this whole file to rebuild the conversation.
/// Returns the record to save, or `None` when nothing changed.
///
/// A file already recorded on the conversation (as its source or an
/// addition) is a repeat of an earlier processing attempt of this same
/// upload, retried on AWS, and is skipped so its messages are never added
/// twice.
async fn merge_into(
    object_store: &dyn ObjectStore,
    user_id: &UserId,
    upload_id: UploadId,
    conversation: &Conversation,
    mut stored: ConversationSummary,
    report: &mut MergeReport,
) -> Result<Option<ConversationSummary>, ProcessingError> {
    if stored.source.upload_id == upload_id || stored.additions.contains(&upload_id) {
        return Ok(None);
    }
    let added: Vec<&ChatMessage> = conversation
        .chat_messages
        .iter()
        .filter(|m| match &stored.message_span {
            None => true,
            Some(range) => {
                let at = m.created_at.fixed_offset();
                at < range.start() || at > range.end()
            }
        })
        .collect();
    let count_after = stored.message_count + added.len();
    if count_after < conversation.chat_messages.len() {
        report.fewer_than_file += 1;
    }
    if added.is_empty() {
        return Ok(None);
    }
    let key = addition_object_key(user_id, stored.conversation_id, upload_id);
    // Unreachable backstop: a list of messages that were just parsed from
    // JSON serializes back to JSON.
    let bytes = serde_json::to_vec(&added).expect("parsed messages serialize to JSON");
    object_store.put(&key, bytes).await?;

    let added_conversation = Conversation {
        uuid: conversation.uuid,
        name: conversation.name.clone(),
        chat_messages: added.into_iter().cloned().collect(),
        extra: serde_json::Map::new(),
    };
    // Unreachable backstop: `added` is non-empty, so it has a span.
    let added_span = message_span(&added_conversation).expect("added messages have times");
    report.gained_messages += 1;
    let start = added_span.start().with_timezone(&chrono::Utc);
    let end = added_span.end().with_timezone(&chrono::Utc);
    report.earliest_added = Some(report.earliest_added.map_or(start, |e| e.min(start)));
    report.latest_added = Some(report.latest_added.map_or(end, |l| l.max(end)));

    stored.additions.push(upload_id);
    stored.message_count = count_after;
    stored.message_span = Some(match &stored.message_span {
        None => added_span,
        Some(range) => range.widened_to(&added_span),
    });
    if stored.span_origin == MetadataOrigin::Guessed {
        stored.span = stored.span.widened_to(&added_span);
    }
    Ok(Some(stored))
}
