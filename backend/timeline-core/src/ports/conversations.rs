//! Conversation summary storage — read access powers `GET /conversations`
//! without ever touching the raw blob in `ObjectStore`, per the migration
//! plan §1.3; the one write method (`put`) is used exclusively by the
//! upload-processing pipeline (see the migration plan §V2a and
//! `timeline-api::processing`), the same "one trait, read+write, only the
//! processing path ever calls the write half" shape
//! [`crate::ports::uploads::UploadOutcomeStore`] already uses — unlike the
//! flag ports, there's no per-actor security boundary here to enforce
//! structurally (only the pipeline ever produces conversation summaries),
//! so a stricter reader/writer split would be ceremony without a
//! corresponding guarantee.
//!
//! Named `ConversationSummaryStore`, not `ConversationStore`: it stores
//! `ConversationSummary` records (name, message count, a foreign key) —
//! never a conversation's actual messages, which are never persisted as a
//! structured thing at all, only reconstructed by re-parsing the raw
//! upload blob on demand (see the migration plan's §V2a-revision). "Store"
//! without qualification would claim more than this trait actually does.
//!
//! Named `put`, not `create`: every adapter's implementation is an upsert
//! (overwrites silently if the key already exists — a plain `HashMap`
//! insert in the in-memory adapter, DynamoDB's own `put_item` in the real
//! one), not "insert, fail if it already exists." `put` also matches
//! `crate::ports::object_store::ObjectStore::put`'s naming for the same
//! upsert shape. This isn't a factory method either — no `ConversationSummary`
//! gets constructed here; the caller (`timeline-api::processing`) already
//! built it and just needs it persisted.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::{UploadId, UserId};
use crate::conversation_metadata::{
    ConversationMedium, ConversationSpan, MetadataOrigin, Participants, SourceFile,
};
use crate::model::{ConversationId, ConversationName};

/// The one record kept per conversation. Despite the name it is now more
/// than a summary: besides the name and message count it records where the
/// conversation's messages are (its first file and any later files that
/// added messages) and its metadata (plan
/// `docs/plans/2026-10-05-screen-flow.md` §8a). Renaming it would touch
/// every user of it for no change in behaviour, so the name stays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: ConversationId,
    pub name: ConversationName,
    /// The first file the conversation came in.
    pub source: SourceFile,
    /// Later files that added messages to it, oldest first; each one's
    /// added messages are stored on their own (plan §8b-2).
    pub additions: Vec<UploadId>,
    pub message_count: usize,
    /// From the earliest to the latest message time; `None` when no
    /// message has a time. Decides which messages a later file adds.
    pub message_span: Option<ConversationSpan>,
    pub participants: Participants,
    pub medium: ConversationMedium,
    /// Whether `participants` and `medium` are still guessed.
    pub details_origin: MetadataOrigin,
    /// The start and end the user sees and edits; places a conversation
    /// whose messages have no times.
    pub span: ConversationSpan,
    pub span_origin: MetadataOrigin,
}

#[async_trait]
pub trait ConversationSummaryStore: Send + Sync {
    async fn list_for_user(&self, user_id: &UserId)
        -> Result<Vec<ConversationSummary>, StoreError>;

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError>;

    /// Writes (upserts) one conversation's summary — called once per
    /// conversation by the upload-processing pipeline, never by a
    /// user-facing route.
    async fn put(&self, user_id: &UserId, summary: ConversationSummary) -> Result<(), StoreError>;
}
