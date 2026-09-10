//! Conversation summary storage — read access powers `GET /conversations`
//! without ever touching the raw S3 blob, per the migration plan §1.3; the
//! one write method (`create`) is used exclusively by the upload-processing
//! pipeline (see the migration plan §V2a and `timeline-api::processing`),
//! the same "one trait, read+write, only the processing path ever calls the
//! write half" shape [`crate::ports::uploads::UploadStore`] already uses —
//! unlike the flag ports, there's no per-actor security boundary here to
//! enforce structurally (only the pipeline ever produces conversation
//! summaries), so a stricter reader/writer split would be ceremony without
//! a corresponding guarantee.

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
pub trait ConversationStore: Send + Sync {
    async fn list_for_user(&self, user_id: &UserId)
        -> Result<Vec<ConversationSummary>, StoreError>;

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError>;

    /// Writes one conversation's summary — called once per conversation by
    /// the upload-processing pipeline, never by a user-facing route.
    async fn create(&self, user_id: &UserId, summary: ConversationSummary)
        -> Result<(), StoreError>;
}
