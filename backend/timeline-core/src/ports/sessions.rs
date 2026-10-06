//! Stored sessions (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md`
//! §3, §6): one row per session, keyed by conversation and number, carrying
//! the fourteen counts that let the timeline, the flag filter and three
//! analyses work without reading messages.

use async_trait::async_trait;

use super::errors::StoreError;
use super::ids::UserId;
use crate::model::ConversationId;
use crate::stored_session::{SessionKey, StoredSession};

#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Every session of the user's, in key order (conversation, then
    /// number).
    async fn list_sessions(&self, user_id: &UserId) -> Result<Vec<StoredSession>, StoreError>;

    /// Up to `max` sessions in key order, starting after `after`: one part
    /// of a reply in parts (§8c).
    async fn sessions_page(
        &self,
        user_id: &UserId,
        after: Option<SessionKey>,
        max: usize,
    ) -> Result<Vec<StoredSession>, StoreError>;

    /// One conversation's sessions, in number order.
    async fn sessions_of(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<StoredSession>, StoreError>;

    /// Writes each session, replacing any with the same key.
    async fn put_sessions(
        &self,
        user_id: &UserId,
        sessions: &[StoredSession],
    ) -> Result<(), StoreError>;

    /// Removes these sessions; a key with no row is not an error.
    async fn delete_sessions(
        &self,
        user_id: &UserId,
        keys: &[SessionKey],
    ) -> Result<(), StoreError>;
}
