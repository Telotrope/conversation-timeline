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
//! `ConversationSummary` records (name, counts, metadata) — never a
//! conversation's messages, which are their own rows
//! ([`crate::ports::messages`]).
//!
//! **Writes are versioned** (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7, C1). Two
//! files of one batch are processed at the same time on AWS; if both hold
//! the same conversation, both would read its record and write it back, the
//! second silently replacing the first. So `put` writes only if the stored
//! record's version is still the one the writer read, and refuses with
//! [`StoreError::Conflict`] otherwise; the writer re-reads and redoes.

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
/// conversation's messages came from (its first file and any later files
/// that added messages) and its metadata (plan
/// `docs/plans/2026-10-05-screen-flow.md` §8a). Renaming it would touch
/// every user of it for no change in behaviour, so the name stays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: ConversationId,
    pub name: ConversationName,
    /// The stored record's version: 0 for a record never written, raised by
    /// one on every write. A writer passes back the version it read.
    pub version: u64,
    /// The first file the conversation came in.
    pub source: SourceFile,
    /// Later files that added messages to it, oldest first.
    pub additions: Vec<UploadId>,
    pub message_count: usize,
    /// Messages whose time the export didn't give (§4e).
    pub untimed: usize,
    /// Messages timed earlier than the timed message before them in the
    /// file: bad data the user is told about on Describe (plan §12.3).
    pub out_of_order: usize,
    /// From the earliest to the latest known message time; `None` when no
    /// message has a time. Decides which messages a later file adds.
    pub message_span: Option<ConversationSpan>,
    pub participants: Participants,
    pub medium: ConversationMedium,
    /// Whether `participants` and `medium` are still guessed.
    pub details_origin: MetadataOrigin,
    /// The start and end the user sees and edits; places a conversation
    /// with messages of unknown time.
    pub span: ConversationSpan,
    pub span_origin: MetadataOrigin,
    /// For a replaced branch kept as a conversation of its own (§4d), the
    /// conversation it branched from.
    pub branch_of: Option<ConversationId>,
    /// The branches of this conversation kept as conversations of their own.
    pub branches: Vec<ConversationId>,
}

#[async_trait]
pub trait ConversationSummaryStore: Send + Sync {
    /// Every record of the user's, in no promised order.
    async fn list_for_user(&self, user_id: &UserId)
        -> Result<Vec<ConversationSummary>, StoreError>;

    /// Up to `max` records in id order, starting after `after`: one part of
    /// a reply in parts (§8c).
    async fn list_page(
        &self,
        user_id: &UserId,
        after: Option<ConversationId>,
        max: usize,
    ) -> Result<Vec<ConversationSummary>, StoreError>;

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError>;

    /// Writes `summary` if the stored record's version is still
    /// `summary.version` (no record at all for version 0), and returns it as
    /// stored, with its version raised by one. A stale version is refused
    /// with [`StoreError::Conflict`] and nothing is written.
    async fn put(
        &self,
        user_id: &UserId,
        summary: ConversationSummary,
    ) -> Result<ConversationSummary, StoreError>;
}
