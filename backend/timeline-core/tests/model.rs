//! Black-box tests for the export schema types — both the round-trip
//! guarantee for fields this crate doesn't interpret, and (per CLAUDE.md's
//! "Type your data" rule) that structured fields are actually validated at
//! parse time: a malformed UUID or timestamp must fail deserialization here,
//! not pass through silently to be discovered later downstream.

use timeline_core::{ChatMessage, Conversation};

/// Fields this crate never reads (files, parent_message_uuid, account,
/// summary, updated_at, ...) must still round-trip, or a Rust backend would
/// silently lose data the browser frontend still expects.
#[test]
fn chat_message_round_trips_unknown_fields() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
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
        "uuid": "6f3a1e2b-2222-4444-8888-0123456789ab",
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
    let raw = serde_json::json!({"uuid": "6f3a1e2b-2222-4444-8888-0123456789ab", "name": "x"});
    let c: Conversation = serde_json::from_value(raw).unwrap();
    assert!(c.chat_messages.is_empty());
}

#[test]
fn conversation_defaults_missing_name_to_empty_string() {
    let raw = serde_json::json!({"uuid": "6f3a1e2b-2222-4444-8888-0123456789ab"});
    let c: Conversation = serde_json::from_value(raw).unwrap();
    assert_eq!(c.name.0, "");
}

/// An unrecognized `sender` value must not fail parsing (Anthropic could add
/// a new one at any time) and must still round-trip to its original text —
/// the `Other` catchall, not a hard error.
#[test]
fn unrecognized_sender_round_trips_via_the_catchall() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "sender": "system",
        "created_at": "2026-01-01T00:00:00Z",
    });
    let m: ChatMessage = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(m.sender, timeline_core::Sender::Other("system".to_string()));
    let back = serde_json::to_value(&m).unwrap();
    assert_eq!(back["sender"], "system");
}

/// Same guarantee for an unrecognized content-piece `type` — a future
/// Anthropic content-block type must not break parsing or lose its value.
#[test]
fn unrecognized_piece_type_round_trips_via_the_catchall() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "text": "",
        "sender": "assistant",
        "created_at": "2026-01-01T00:00:00Z",
        "content": [{"type": "image", "text": "", "source": {"type": "base64"}}],
    });
    let m: ChatMessage = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(
        m.content[0].piece_type,
        timeline_core::PieceType::Other("image".to_string())
    );
    let back = serde_json::to_value(&m).unwrap();
    assert_eq!(back, raw);
}

/// The concrete "test of parsing objects": a malformed UUID must fail
/// deserialization at the boundary — not pass through as an opaque string
/// to be discovered wrong later, wherever a `uuid` field happens to get
/// used.
#[test]
fn malformed_uuid_fails_to_parse() {
    let raw = serde_json::json!({
        "uuid": "not-a-valid-uuid",
        "sender": "human",
        "created_at": "2026-01-01T00:00:00Z",
    });
    let err = serde_json::from_value::<ChatMessage>(raw).unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("uuid"),
        "error should mention the uuid problem: {err}"
    );
}

/// Same guarantee for `created_at`: this is the field that used to be
/// validated lazily, deep inside `sessions::build_blocks`, only when a
/// caller happened to ask for session blocks. It's validated here instead,
/// at the only place raw JSON enters the crate.
#[test]
fn malformed_timestamp_fails_to_parse() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "sender": "human",
        "created_at": "not-a-real-timestamp",
    });
    let err = serde_json::from_value::<ChatMessage>(raw).unwrap_err();
    assert!(!err.to_string().is_empty());
}

/// Rewritten as approved in plan
/// 2026-10-06-load-only-what-the-page-shows.md §10b (it was
/// `missing_timestamp_fails_to_parse`): a missing `created_at`, or `null`,
/// is read with the time unknown (§4e), written down as the zero date and
/// left out again when the message is written back. A malformed one is
/// still refused (`malformed_timestamp_fails_to_parse`, unchanged).
#[test]
fn a_missing_or_null_timestamp_is_read_as_unknown_and_left_out_when_written() {
    for raw in [
        serde_json::json!({
            "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
            "sender": "human",
        }),
        serde_json::json!({
            "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
            "sender": "human",
            "created_at": null,
        }),
    ] {
        let message = serde_json::from_value::<ChatMessage>(raw).unwrap();
        assert_eq!(message.time(), timeline_core::MessageTime::Unknown);
        assert_eq!(message.created_at, timeline_core::UNKNOWN_TIME);
        let written = serde_json::to_value(&message).unwrap();
        assert!(written.get("created_at").is_none(), "{written}");
    }
}

#[test]
fn a_known_timestamp_reads_back_as_known_and_is_written_out() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "sender": "human",
        "created_at": "2026-01-01T00:00:00Z",
    });
    let message = serde_json::from_value::<ChatMessage>(raw).unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    assert_eq!(message.time(), timeline_core::MessageTime::Known(at));
    assert_eq!(message.time().known(), Some(at));
    assert_eq!(message.time().written(), at);
    assert_eq!(
        serde_json::to_value(&message).unwrap()["created_at"],
        "2026-01-01T00:00:00Z"
    );
    assert_eq!(timeline_core::MessageTime::Unknown.known(), None);
    assert_eq!(
        timeline_core::MessageTime::Unknown.written(),
        timeline_core::UNKNOWN_TIME
    );
}

/// `sender` being present but the wrong JSON type (a number, not a string)
/// must fail parsing rather than being silently coerced — this is the
/// catchall's error path, distinct from the "unrecognized but still a
/// string" case tested above.
#[test]
fn sender_of_the_wrong_json_type_fails_to_parse() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "sender": 42,
        "created_at": "2026-01-01T00:00:00Z",
    });
    assert!(serde_json::from_value::<ChatMessage>(raw).is_err());
}

/// Same guarantee for a content piece's `type` field.
#[test]
fn piece_type_of_the_wrong_json_type_fails_to_parse() {
    let raw = serde_json::json!({
        "uuid": "3f846bbc-6941-49df-b8cf-9864e7d7dcea",
        "sender": "human",
        "created_at": "2026-01-01T00:00:00Z",
        "content": [{"type": null, "text": "hi"}],
    });
    assert!(serde_json::from_value::<ChatMessage>(raw).is_err());
}
