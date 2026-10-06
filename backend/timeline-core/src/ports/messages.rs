//! The stored rows of conversations' messages and branch notes, with your
//! messages' flags on them (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3). Each row is
//! keyed by conversation, time and id, so one session's rows are one
//! unbroken run of keys, read with one range query.
//!
//! Writing is split into narrow traits, as the `MessageFlags` port this
//! replaces was, so that the automatic/yours separation of
//! [timeline-project-decisions.md section 2.6](../../../../timeline-project-decisions.md#L98)
//! is enforced by the types: the scan holds an [`AutoFlagWriter`], which
//! can only write the automatic flags; a flag save holds a
//! [`UserFlagWriter`], which can only write yours; and only processing
//! holds a [`MessageRowWriter`], which writes whole rows. On a row the two
//! kinds of flag are separate attributes, so neither writer can overwrite
//! the other's.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::errors::StoreError;
use super::ids::UserId;
use crate::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use crate::model::{ConversationId, MessageId};
use crate::stored_message::{Entry, EntryKey};
use crate::stored_session::{Placement, StoredSession};

/// Which of a conversation's rows to read, in key order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryRange {
    pub conversation_id: ConversationId,
    /// Only rows timed from `from` to `to`, ends included; `None` for the
    /// whole conversation.
    pub times: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Only rows after this key: where an earlier part stopped.
    pub after: Option<EntryKey>,
}

impl EntryRange {
    /// Every row of one session: a range of times, or for a session placed
    /// by its conversation's start and end, the whole conversation (§4e).
    pub fn session(session: &StoredSession) -> Self {
        Self {
            conversation_id: session.conversation_id,
            times: match session.placement {
                Placement::Gaps => Some((session.start, session.end)),
                Placement::Span => None,
            },
            after: None,
        }
    }
}

#[async_trait]
pub trait MessageReader: Send + Sync {
    /// The rows in `range`, in key order.
    async fn read_entries(
        &self,
        user_id: &UserId,
        range: EntryRange,
    ) -> Result<Vec<Entry>, StoreError>;

    /// The row of the message with this id in this conversation, found
    /// among the conversation's rows; `None` when there is none.
    async fn find_entry(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<Entry>, StoreError>;

    /// The row that follows `key` in its conversation: a message's reply.
    async fn entry_after(
        &self,
        user_id: &UserId,
        key: EntryKey,
    ) -> Result<Option<Entry>, StoreError>;
}

/// Whole-row writes: held by upload processing only.
#[async_trait]
pub trait MessageRowWriter: Send + Sync {
    /// Writes every entry, replacing any row with the same key, flags
    /// included: your message's flags as the entry carries them.
    /// A write DynamoDB leaves unfinished is retried; rows still unwritten
    /// after every retry are reported as [`StoreError::Unwritten`].
    async fn put_entries(&self, user_id: &UserId, entries: &[Entry]) -> Result<(), StoreError>;

    /// Removes these rows; a key with no row is not an error.
    async fn delete_entries(&self, user_id: &UserId, keys: &[EntryKey]) -> Result<(), StoreError>;
}

/// Write access to *only* the automatic flags. Held by the scan.
#[async_trait]
pub trait AutoFlagWriter: Send + Sync {
    /// Replaces the message's automatic flags; your flags are untouched. A
    /// key with no row is [`StoreError::NotFound`], and nothing is created.
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        flags: FlagSet,
    ) -> Result<(), StoreError>;
}

/// Write access to *only* your flags. Held by the flag-save route.
#[async_trait]
pub trait UserFlagWriter: Send + Sync {
    /// Sets the flags `overrides` names and leaves the others, returning
    /// the message's flags as stored. A key with no row is
    /// [`StoreError::NotFound`], and nothing is created.
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        overrides: FlagOverrides,
    ) -> Result<MessageFlags, StoreError>;
}
