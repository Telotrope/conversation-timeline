//! Minimal, round-trip-safe representation of an Anthropic `conversations.json`
//! export. Every field this crate doesn't interpret is preserved verbatim in
//! `extra` (via `#[serde(flatten)]`) so a message or conversation can be
//! re-serialized without silently dropping data the frontend still needs.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentPiece {
    #[serde(rename = "type")]
    pub piece_type: String,
    #[serde(default)]
    pub text: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub uuid: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub content: Vec<ContentPiece>,
    pub sender: String,
    pub created_at: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub uuid: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub chat_messages: Vec<ChatMessage>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}
