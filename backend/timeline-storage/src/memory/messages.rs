//! In-memory message rows. Implements every message trait on one struct for
//! convenience of test and local setup; the automatic/yours separation is
//! still real, because a route is only ever handed the one narrow trait it
//! needs (see `timeline-api`), never this struct.

use std::collections::BTreeMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{
    AutoFlagWriter, EntryRange, MessageReader, MessageRowWriter, UserFlagWriter,
};
use timeline_core::stored_message::{Entry, EntryKey};

type Rows = BTreeMap<(UserId, EntryKey), Entry>;

#[derive(Default)]
pub struct InMemoryMessageStore {
    rows: Mutex<Rows>,
}

impl InMemoryMessageStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn rows(&self) -> std::sync::MutexGuard<'_, Rows> {
        self.rows.lock().expect("in-memory store mutex poisoned")
    }
}

fn in_range(key: &EntryKey, range: &EntryRange) -> bool {
    key.conversation_id == range.conversation_id
        && range
            .positions
            .is_none_or(|(first, last)| first <= key.position && key.position <= last)
        && range.after.is_none_or(|after| *key > after)
}

/// The flags of your message's row under `key`, for a flag write to
/// change. Only your messages carry flags (processing gives each one, even
/// an unflagged one); Claude's messages and notes have none to change.
fn flags_of<'a>(
    rows: &'a mut Rows,
    user_id: &UserId,
    key: EntryKey,
) -> Option<&'a mut MessageFlags> {
    match rows.get_mut(&(user_id.clone(), key)) {
        Some(Entry::Message(m)) => m.flags.as_mut(),
        _ => None,
    }
}

#[async_trait]
impl MessageReader for InMemoryMessageStore {
    async fn read_entries(
        &self,
        user_id: &UserId,
        range: EntryRange,
    ) -> Result<Vec<Entry>, StoreError> {
        Ok(self
            .rows()
            .iter()
            .filter(|((uid, key), _)| uid == user_id && in_range(key, &range))
            .map(|(_, entry)| entry.clone())
            .collect())
    }

    async fn find_entry(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<Entry>, StoreError> {
        Ok(self
            .rows()
            .iter()
            .find(|((uid, key), entry)| {
                uid == user_id
                    && key.conversation_id == conversation_id
                    && key.id == message_id
                    && entry.as_message().is_some()
            })
            .map(|(_, entry)| entry.clone()))
    }

    async fn entry_after(
        &self,
        user_id: &UserId,
        key: EntryKey,
    ) -> Result<Option<Entry>, StoreError> {
        Ok(self
            .rows()
            .range((user_id.clone(), key)..)
            .find(|((uid, k), _)| uid == user_id && *k > key)
            .filter(|((_, k), _)| k.conversation_id == key.conversation_id)
            .map(|(_, entry)| entry.clone()))
    }
}

#[async_trait]
impl MessageRowWriter for InMemoryMessageStore {
    async fn put_entries(&self, user_id: &UserId, entries: &[Entry]) -> Result<(), StoreError> {
        let mut rows = self.rows();
        for entry in entries {
            rows.insert((user_id.clone(), entry.key()), entry.clone());
        }
        Ok(())
    }

    async fn delete_entries(&self, user_id: &UserId, keys: &[EntryKey]) -> Result<(), StoreError> {
        let mut rows = self.rows();
        for key in keys {
            rows.remove(&(user_id.clone(), *key));
        }
        Ok(())
    }
}

#[async_trait]
impl AutoFlagWriter for InMemoryMessageStore {
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        flags: FlagSet,
    ) -> Result<(), StoreError> {
        let mut rows = self.rows();
        let stored = flags_of(&mut rows, user_id, key).ok_or(StoreError::NotFound)?;
        stored.auto = Some(flags);
        Ok(())
    }
}

#[async_trait]
impl UserFlagWriter for InMemoryMessageStore {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        overrides: FlagOverrides,
    ) -> Result<MessageFlags, StoreError> {
        let mut rows = self.rows();
        let stored = flags_of(&mut rows, user_id, key).ok_or(StoreError::NotFound)?;
        stored.user = stored.user.updated_by(overrides);
        Ok(*stored)
    }
}

impl crate::memory::resettable::Resettable for InMemoryMessageStore {
    fn reset(&self) {
        self.rows().clear();
    }
}
