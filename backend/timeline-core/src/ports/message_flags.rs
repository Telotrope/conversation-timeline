//! Per-message flag storage, split into three narrow traits specifically so
//! the auto/user separation from
//! [timeline-project-decisions.md section 2.6](../../../../timeline-project-decisions.md#L98)
//! is enforced by the type system, not just by convention: a route handler
//! is only ever given the one capability it needs -- for example, the
//! `PATCH /conversations/{conversation_id}/messages/{message_id}/flags`
//! handler is constructed with a [`UserFlagWriter`] and has no way to call
//! anything that would touch an auto-detected value, because no such method
//! exists on the trait it holds. See the migration plan section 4.1 and
//! section 6.2 ("ports and adapters").
//!
//! Every method takes both a conversation id and a message id, matching the
//! `MessageFlags` DynamoDB table's key design from the plan's section 1.3
//! (`PK user_id#conversation_id, SK message_id`) -- a message id alone
//! doesn't carry enough information to build that key. An earlier draft of
//! this port and its route only carried a message id; that mismatch was
//! caught while writing the DynamoDB adapter, and fixed here rather than
//! carried forward into the storage layer.

use async_trait::async_trait;

use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::UserId;
use crate::model::{ConversationId, MessageId};

/// The three flag types this tool has ever had -- see
/// [timeline-project-decisions.md section 5](../../../../timeline-project-decisions.md#L216)
/// for why these three, and why caps is mechanical while the other two are
/// heuristic judgment calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlagSet {
    pub caps: bool,
    pub critical: bool,
    pub angry: bool,
}

/// A partial update to a message's user-owned flag overrides -- `None`
/// means "leave this flag's override untouched," not "clear it." Matches
/// the original UI's "click one checkbox" (one `Some`) and "click Approve"
/// (all three `Some`, computed client-side from the current effective
/// values) cases from
/// [timeline-project-decisions.md section 5.3](../../../../timeline-project-decisions.md#L253).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlagOverrides {
    pub caps: Option<bool>,
    pub critical: Option<bool>,
    pub angry: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageFlagRecord {
    pub message_id: MessageId,
    pub auto: FlagSet,
    pub user: FlagOverrides,
}

#[async_trait]
pub trait MessageFlagsReader: Send + Sync {
    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<MessageFlagRecord>, StoreError>;

    /// Every flag record for one conversation, in one call -- the Review
    /// tab's real access pattern (show every message's flags at once), not
    /// one message at a time.
    async fn list_for_conversation(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<MessageFlagRecord>, StoreError>;
}

/// Write access to *only* the auto-detected flags. Held exclusively by the
/// upload-processing pipeline (the heuristic pass today, Bedrock in V3) --
/// never by anything reachable from an authenticated user's own request.
#[async_trait]
pub trait AutoFlagWriter: Send + Sync {
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        flags: FlagSet,
    ) -> Result<(), StoreError>;
}

/// Write access to *only* the user's own overrides. Held exclusively by the
/// `PATCH .../flags` route handler.
#[async_trait]
pub trait UserFlagWriter: Send + Sync {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        overrides: FlagOverrides,
    ) -> Result<MessageFlagRecord, StoreError>;
}
