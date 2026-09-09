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

#[cfg(test)]
mod tests {
    use super::*;

    /// Fields this crate never reads (attachments, files, parent_message_uuid,
    /// account, summary, updated_at, ...) must still round-trip, or a Rust
    /// backend would silently lose data the browser frontend still expects.
    #[test]
    fn chat_message_round_trips_unknown_fields() {
        let raw = serde_json::json!({
            "uuid": "m1",
            "text": "hi",
            "content": [{"type": "text", "text": "hi", "start_timestamp": "2026-01-01T00:00:00Z"}],
            "sender": "human",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "attachments": [],
            "files": [],
            "parent_message_uuid": "00000000-0000-4000-8000-000000000000",
        });
        let m: ChatMessage = serde_json::from_value(raw.clone()).unwrap();
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn conversation_round_trips_unknown_fields() {
        let raw = serde_json::json!({
            "uuid": "c1",
            "name": "a conversation",
            "summary": "",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "account": {"uuid": "acct"},
            "chat_messages": [],
        });
        let c: Conversation = serde_json::from_value(raw.clone()).unwrap();
        let back = serde_json::to_value(&c).unwrap();
        assert_eq!(back, raw);
    }

    #[test]
    fn conversation_defaults_missing_chat_messages_to_empty() {
        let raw = serde_json::json!({"uuid": "c1", "name": "x"});
        let c: Conversation = serde_json::from_value(raw).unwrap();
        assert!(c.chat_messages.is_empty());
    }
}
