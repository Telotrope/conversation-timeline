//! In-memory `ConversationStore`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationStore, ConversationSummary};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;

#[derive(Default)]
pub struct InMemoryConversationStore {
    summaries: Mutex<HashMap<(UserId, ConversationId), ConversationSummary>>,
}

impl InMemoryConversationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Not part of the `ConversationStore` trait — this is how the upload
    /// processing step (or a test standing in for it) populates summaries
    /// once an upload has been parsed, the same way a real adapter would
    /// batch-write conversation-summary items to DynamoDB.
    pub fn insert(&self, user_id: UserId, summary: ConversationSummary) {
        self.summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert((user_id, summary.conversation_id), summary);
    }
}

#[async_trait]
impl ConversationStore for InMemoryConversationStore {
    async fn list_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        Ok(self
            .summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .iter()
            .filter(|((uid, _), _)| uid == user_id)
            .map(|(_, summary)| summary.clone())
            .collect())
    }

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError> {
        Ok(self
            .summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), conversation_id))
            .cloned())
    }
}
