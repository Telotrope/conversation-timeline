//! In-memory `ConversationSummaryStore`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;

#[derive(Default)]
pub struct InMemoryConversationSummaryStore {
    summaries: Mutex<BTreeMap<(UserId, ConversationId), ConversationSummary>>,
}

impl InMemoryConversationSummaryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test-only convenience: populates a summary directly, as stored,
    /// without the version check of `ConversationSummaryStore::put`; this
    /// inherent method exists so synchronous test setup doesn't need a
    /// runtime just to seed a store.
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

    async fn list_page(
        &self,
        user_id: &UserId,
        after: Option<ConversationId>,
        max: usize,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        Ok(self
            .summaries
            .lock()
            .expect("in-memory store mutex poisoned")
            .iter()
            .filter(|((uid, id), _)| uid == user_id && after.is_none_or(|a| *id > a))
            .take(max)
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
        mut summary: ConversationSummary,
    ) -> Result<ConversationSummary, StoreError> {
        let mut summaries = self
            .summaries
            .lock()
            .expect("in-memory store mutex poisoned");
        let key = (user_id.clone(), summary.conversation_id);
        let stored_version = summaries.get(&key).map_or(0, |s| s.version);
        if stored_version != summary.version {
            return Err(StoreError::Conflict);
        }
        summary.version += 1;
        summaries.insert(key, summary.clone());
        Ok(summary)
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
