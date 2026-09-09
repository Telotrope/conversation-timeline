//! Black-box round-trip tests for the export schema types.

use timeline_core::{ChatMessage, Conversation};

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
