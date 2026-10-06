//! The user's own record (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c, §8b): the
//! data version, raised by every upload, flag save and scan, which tells a
//! saved analysis or a reply in parts whether the data changed under it; and
//! the totals a progress bar measures against.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::UserId;

/// How much the user has: what "done of total" counts against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Totals {
    pub conversations: usize,
    pub sessions: usize,
    /// Your messages.
    pub your_messages: usize,
    /// Every message, yours and Claude's.
    pub messages: usize,
}

/// The user's record. A user who has stored nothing has version 0 and no
/// totals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserRecord {
    pub data_version: u64,
    pub totals: Totals,
}

#[async_trait]
pub trait UserRecordStore: Send + Sync {
    async fn get(&self, user_id: &UserId) -> Result<UserRecord, StoreError>;

    /// Raises the data version by one and returns the record as stored.
    /// Applied by the store itself, so two writers can't both read the old
    /// version.
    async fn raise_version(&self, user_id: &UserId) -> Result<UserRecord, StoreError>;

    /// Replaces the totals, raises the data version by one, and returns the
    /// record as stored. Processing counts the totals afresh after every
    /// upload, so they can't drift.
    async fn record_totals(
        &self,
        user_id: &UserId,
        totals: Totals,
    ) -> Result<UserRecord, StoreError>;
}
