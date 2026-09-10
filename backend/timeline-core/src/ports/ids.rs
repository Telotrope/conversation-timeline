//! Identity types for the storage ports. Distinct newtypes, not bare
//! `String`/`Uuid`, for the same reason as [`crate::model::MessageId`] —
//! see CLAUDE.md's "Type your data": a `UserId` and an `UploadId` must never
//! be swappable at a call site just because they happen to share a
//! representation.

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A user's identity, as asserted by the auth layer (Cognito's `sub` claim).
/// Opaque `String`, not `Uuid` — identity-provider subject identifiers
/// aren't guaranteed to be UUID-shaped across every possible federated
/// provider, so this only promises "a stable opaque string," not a format.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UserId(pub String);

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies one upload (one `conversations.json` a user submitted),
/// distinct from [`crate::model::ConversationId`] — an upload can contain
/// many conversations, and the two ids must never be interchangeable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UploadId(pub Uuid);

impl fmt::Display for UploadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
