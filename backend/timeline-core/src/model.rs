//! Typed representation of an Anthropic `conversations.json` export.
//! Fields with real structure (identity, a closed set of values, an instant
//! in time) are parsed into dedicated types at deserialization time, not
//! left as bare strings to be re-validated wherever they're used — see
//! CLAUDE.md's "Type your data — avoid primitive obsession". Every field
//! this crate doesn't otherwise interpret is preserved verbatim in `extra`
//! (via `#[serde(flatten)]`) so a message or conversation can still be
//! re-serialized without silently dropping data the frontend still needs.
//!
//! One consequence of parsing `created_at` at this boundary: a message with
//! an unparseable timestamp now fails deserialization outright (surfaced as
//! `FormatError::InvalidConversation`) instead of being silently skipped
//! later, deep inside `sessions::build_blocks`, the way it used to be. If
//! that turns out to be too strict in practice — if real exports commonly
//! contain messages with malformed timestamps — the fix belongs here, at
//! this same boundary (e.g. substituting a fallback value during parsing),
//! not as a second, looser check downstream. That's deliberately not
//! built yet; there's no evidence yet that it's needed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Who sent a message. Anthropic's export format only uses `"human"` and
/// `"assistant"` today; `Other` is a catchall for any future value, per
/// CLAUDE.md's "catchall over enumeration" — not because this crate treats
/// a third sender type specially (it doesn't, yet), but so schema evolution
/// doesn't silently fail deserialization or lose the original value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sender {
    Human,
    Assistant,
    Other(String),
}

impl Sender {
    fn as_str(&self) -> &str {
        match self {
            Sender::Human => "human",
            Sender::Assistant => "assistant",
            Sender::Other(s) => s,
        }
    }
}

impl From<String> for Sender {
    fn from(s: String) -> Self {
        match s.as_str() {
            "human" => Sender::Human,
            "assistant" => Sender::Assistant,
            _ => Sender::Other(s),
        }
    }
}

impl Serialize for Sender {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Sender {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Sender::from(String::deserialize(deserializer)?))
    }
}

/// The `"type"` of a `content` piece. Real exports carry several — `"text"`,
/// `"tool_use"`, `"tool_result"`, `"thinking"`, and Anthropic adds more over
/// time (`"image"`, `"redacted_thinking"`, ...). Only `Text` is ever handled
/// differently by this crate today, so per "catchall over enumeration"
/// everything else collapses into `Other` rather than being individually
/// named — promote a variant out of `Other` only once something actually
/// needs to treat it differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PieceType {
    Text,
    Other(String),
}

impl PieceType {
    fn as_str(&self) -> &str {
        match self {
            PieceType::Text => "text",
            PieceType::Other(s) => s,
        }
    }
}

impl From<String> for PieceType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "text" => PieceType::Text,
            _ => PieceType::Other(s),
        }
    }
}

impl Serialize for PieceType {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for PieceType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(PieceType::from(String::deserialize(deserializer)?))
    }
}

/// A message's own identity. Wraps `uuid::Uuid` (parsed and validated at
/// deserialization time) rather than a bare `String`, and is a distinct
/// type from [`ConversationId`] specifically so the two can't be swapped at
/// a call site that takes both — the newtype pattern costs nothing at
/// runtime and turns that mistake into a compile error instead of a bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(pub uuid::Uuid);

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A conversation's own identity — see [`MessageId`] for why this is a
/// distinct type rather than reusing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationId(pub uuid::Uuid);

impl std::fmt::Display for ConversationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A conversation's title. Freeform text a human or Claude chose, so there's
/// nothing to validate or parse — but it identifies a conversation rather
/// than being content that gets read/processed, so it's still wrapped
/// rather than left as a bare `String` indistinguishable from any other.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConversationName(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentPiece {
    #[serde(rename = "type")]
    pub piece_type: PieceType,
    #[serde(default)]
    pub text: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub uuid: MessageId,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub content: Vec<ContentPiece>,
    pub sender: Sender,
    pub created_at: DateTime<Utc>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub uuid: ConversationId,
    #[serde(default)]
    pub name: ConversationName,
    #[serde(default)]
    pub chat_messages: Vec<ChatMessage>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
