//! In-memory message-flags store. Implements all three flag traits on one
//! struct for convenience of test/local setup -- the auto/user separation is
//! still real, because a route handler is only ever handed a reference
//! typed as the one narrow trait it needs (see `timeline-api`), never this
//! concrete struct directly.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::message_flags::{
    AutoFlagWriter, FlagOverrides, FlagSet, MessageFlagRecord, MessageFlagsReader, UserFlagWriter,
};

type Key = (UserId, ConversationId, MessageId);

#[derive(Default)]
pub struct InMemoryMessageFlagsStore {
    records: Mutex<HashMap<Key, MessageFlagRecord>>,
}

impl InMemoryMessageFlagsStore {
    pub fn new() -> Self {
        Self::default()
    }
}

fn blank(message_id: MessageId) -> MessageFlagRecord {
    MessageFlagRecord {
        message_id,
        auto: FlagSet::default(),
        user: FlagOverrides::default(),
    }
}

#[async_trait]
impl MessageFlagsReader for InMemoryMessageFlagsStore {
    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<MessageFlagRecord>, StoreError> {
        Ok(self
            .records
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), conversation_id, message_id))
            .copied())
    }

    async fn list_for_conversation(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<MessageFlagRecord>, StoreError> {
        Ok(self
            .records
            .lock()
            .expect("in-memory store mutex poisoned")
            .iter()
            .filter(|((uid, cid, _), _)| uid == user_id && *cid == conversation_id)
            .map(|(_, record)| *record)
            .collect())
    }
}

#[async_trait]
impl AutoFlagWriter for InMemoryMessageFlagsStore {
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        flags: FlagSet,
    ) -> Result<(), StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records
            .entry((user_id.clone(), conversation_id, message_id))
            .or_insert_with(|| blank(message_id));
        record.auto = flags;
        Ok(())
    }
}

#[async_trait]
impl UserFlagWriter for InMemoryMessageFlagsStore {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        overrides: FlagOverrides,
    ) -> Result<MessageFlagRecord, StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records
            .entry((user_id.clone(), conversation_id, message_id))
            .or_insert_with(|| blank(message_id));
        if let Some(caps) = overrides.caps {
            record.user.caps = Some(caps);
        }
        if let Some(critical) = overrides.critical {
            record.user.critical = Some(critical);
        }
        if let Some(angry) = overrides.angry {
            record.user.angry = Some(angry);
        }
        Ok(*record)
    }
}
