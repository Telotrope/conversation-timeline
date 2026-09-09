//! Black-box tests for `unwrap_uploaded_json`, calling only the crate's
//! public API.

use std::error::Error;

use timeline_core::{unwrap_uploaded_json, FormatError};

#[test]
fn bare_array_is_not_already_processed_and_gets_deduped() {
    let raw = serde_json::json!([
        {
            "uuid": "c0",
            "name": "conv",
            "chat_messages": [
                {"uuid": "m0", "content": [{"type": "text", "text": "hi"}], "sender": "human", "created_at": "t0"},
                {"uuid": "m1", "content": [{"type": "text", "text": "hi"}], "sender": "human", "created_at": "t1"},
            ]
        }
    ])
    .to_string();
    let result = unwrap_uploaded_json(&raw).unwrap();
    assert!(!result.already_processed);
    assert_eq!(
        result.conversations[0].chat_messages.len(),
        1,
        "the duplicate must have been collapsed"
    );
}

#[test]
fn wrapped_object_is_already_processed_and_not_deduped_again() {
    let raw = serde_json::json!({
        "claude_timeline_format_version": "2",
        "conversations": [
            {
                "uuid": "c0",
                "name": "conv",
                "chat_messages": [
                    {"uuid": "m0", "content": [{"type": "text", "text": "hi"}], "sender": "human", "created_at": "t0"},
                    {"uuid": "m1", "content": [{"type": "text", "text": "hi"}], "sender": "human", "created_at": "t1"},
                ]
            }
        ]
    })
    .to_string();
    let result = unwrap_uploaded_json(&raw).unwrap();
    assert!(result.already_processed);
    assert_eq!(
        result.conversations[0].chat_messages.len(),
        2,
        "a wrapped/already-processed file must not be deduped again, even if it still contains would-be duplicates"
    );
}

#[test]
fn neither_shape_is_a_distinct_surfaced_error() {
    let err = unwrap_uploaded_json("{\"not_conversations\": []}").unwrap_err();
    assert!(matches!(err, FormatError::UnrecognizedShape));
    assert_eq!(
        err.to_string(),
        "Expected either a bare array of conversations or a {conversations: [...]} object."
    );
}

#[test]
fn invalid_json_is_a_distinct_surfaced_error() {
    let err = unwrap_uploaded_json("{not json").unwrap_err();
    assert!(matches!(err, FormatError::InvalidJson(_)));
    assert!(err.source().is_some());
    assert!(err.to_string().starts_with("invalid JSON: "));
}

#[test]
fn malformed_conversation_error_exposes_display_and_source() {
    let err = unwrap_uploaded_json(r#"[{"name": "missing uuid and sender"}]"#).unwrap_err();
    assert!(matches!(err, FormatError::InvalidConversation(_)));
    assert!(err.source().is_some());
    assert!(err.to_string().starts_with("invalid conversation data: "));
}

/// Same malformed-conversation error, but reached through the *wrapped*
/// shape's own parsing path (a distinct source location from the bare-array
/// case above) — a file claiming to be already-processed can still contain
/// invalid conversation data.
#[test]
fn malformed_conversation_inside_a_wrapped_object_is_also_a_distinct_surfaced_error() {
    let err = unwrap_uploaded_json(r#"{"conversations": [{"name": "missing uuid and sender"}]}"#)
        .unwrap_err();
    assert!(matches!(err, FormatError::InvalidConversation(_)));
}

#[test]
fn unrecognized_shape_error_has_no_source() {
    let err = unwrap_uploaded_json("42").unwrap_err();
    assert!(matches!(err, FormatError::UnrecognizedShape));
    assert!(err.source().is_none());
}

#[test]
fn conversations_field_not_an_array_is_unrecognized_shape() {
    let err = unwrap_uploaded_json(r#"{"conversations": "not an array"}"#).unwrap_err();
    assert!(matches!(err, FormatError::UnrecognizedShape));
}
