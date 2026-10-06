//! In-memory `SessionStore`.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::ConversationId;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::stored_session::{SessionKey, StoredSession};

type Rows = BTreeMap<(UserId, SessionKey), StoredSession>;

#[derive(Default)]
pub struct InMemorySessionStore {
    rows: Mutex<Rows>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, Rows> {
        self.rows.lock().expect("in-memory store mutex poisoned")
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn list_sessions(&self, user_id: &UserId) -> Result<Vec<StoredSession>, StoreError> {
        Ok(self
            .rows()
            .iter()
            .filter(|((uid, _), _)| uid == user_id)
            .map(|(_, s)| s.clone())
            .collect())
    }

    async fn sessions_page(
        &self,
        user_id: &UserId,
        after: Option<SessionKey>,
        max: usize,
    ) -> Result<Vec<StoredSession>, StoreError> {
        Ok(self
            .rows()
            .iter()
            .filter(|((uid, key), _)| uid == user_id && after.is_none_or(|a| *key > a))
            .take(max)
            .map(|(_, s)| s.clone())
            .collect())
    }

    async fn sessions_of(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<StoredSession>, StoreError> {
        Ok(self
            .rows()
            .iter()
            .filter(|((uid, key), _)| uid == user_id && key.conversation_id == conversation_id)
            .map(|(_, s)| s.clone())
            .collect())
    }

    async fn put_sessions(
        &self,
        user_id: &UserId,
        sessions: &[StoredSession],
    ) -> Result<(), StoreError> {
        let mut rows = self.rows();
        for s in sessions {
            rows.insert((user_id.clone(), s.key()), s.clone());
        }
        Ok(())
    }

    async fn delete_sessions(
        &self,
        user_id: &UserId,
        keys: &[SessionKey],
    ) -> Result<(), StoreError> {
        let mut rows = self.rows();
        for key in keys {
            rows.remove(&(user_id.clone(), *key));
        }
        Ok(())
    }
}

impl crate::memory::resettable::Resettable for InMemorySessionStore {
    fn reset(&self) {
        self.rows().clear();
    }
}
