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
use crate::model::{ConversationId, ConversationName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: ConversationId,
    pub upload_id: UploadId,
    pub name: ConversationName,
    pub message_count: usize,
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
