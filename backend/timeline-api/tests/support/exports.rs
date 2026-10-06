//! Small exports in Claude's format for tests: conversations of messages
//! at chosen minutes, each answering the one before, as a real export's
//! messages do.

#![allow(dead_code)]

use serde_json::{json, Value};

pub const ROOT: &str = "00000000-0000-4000-8000-000000000000";

/// A conversation id from a number.
pub fn conv(n: u32) -> String {
    format!("cccccccc-0000-4000-8000-{n:012}")
}

/// A message id from a conversation number and a message number.
pub fn msg(conversation: u32, n: u32) -> String {
    format!("{:08x}-0000-4000-8000-{n:012}", conversation)
}

/// Minutes after 2026-03-02 09:00 UTC, as the export writes times.
pub fn at(minute: i64) -> String {
    (chrono::DateTime::parse_from_rfc3339("2026-03-02T09:00:00Z").unwrap()
        + chrono::Duration::minutes(minute))
    .with_timezone(&chrono::Utc)
    .to_rfc3339()
}

/// One message: who sent it, when (`None` for no time), its text.
pub struct M<'a> {
    pub sender: &'a str,
    pub minute: Option<i64>,
    pub text: &'a str,
}

pub fn you(minute: i64, text: &str) -> M<'_> {
    M {
        sender: "human",
        minute: Some(minute),
        text,
    }
}

pub fn claude(minute: i64, text: &str) -> M<'_> {
    M {
        sender: "assistant",
        minute: Some(minute),
        text,
    }
}

/// Conversation `n` named `name`, its messages each answering the one
/// before.
pub fn conversation(n: u32, name: &str, messages: &[M]) -> Value {
    let mut parent = ROOT.to_string();
    let list: Vec<Value> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let id = msg(n, i as u32 + 1);
            let mut v = json!({
                "uuid": id,
                "parent_message_uuid": parent,
                "sender": m.sender,
                "content": [{"type": "text", "text": m.text}],
            });
            if let Some(minute) = m.minute {
                v["created_at"] = json!(at(minute));
            }
            parent = id;
            v
        })
        .collect();
    json!({"uuid": conv(n), "name": name, "chat_messages": list})
}

/// An export of these conversations, as text.
pub fn export(conversations: Vec<Value>) -> String {
    Value::Array(conversations).to_string()
}
