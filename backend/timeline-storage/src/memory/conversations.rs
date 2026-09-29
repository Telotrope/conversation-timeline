//! In-memory `ConversationSummaryStore`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;

#[derive(Default)]
pub struct InMemoryConversationSummaryStore {
    summaries: Mutex<HashMap<(UserId, ConversationId), ConversationSummary>>,
}

impl InMemoryConversationSummaryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only convenience: populates a summary directly, without going
    /// through the async trait method. `ConversationSummaryStore::put`
    /// (below) delegates here; this inherent method exists so synchronous
    /// test setup doesn't need a runtime just to seed a store.
    pub fn insert(&self, user_id: UserId, summary: ConversationSummary) {
        self.summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert((user_id, summary.conversation_id), summary);
    }
}

#[async_trait]
impl ConversationSummaryStore for InMemoryConversationSummaryStore {
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

    async fn put(
        &self,
        user_id: &UserId,
        summary: ConversationSummary,
    ) -> Result<(), StoreError> {
        self.insert(user_id.clone(), summary);
        Ok(())
    }
}

impl crate::memory::resettable::Resettable for InMemoryConversationSummaryStore {
    fn reset(&self) {
        self.summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .clear();
    }
}
