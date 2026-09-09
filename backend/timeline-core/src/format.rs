//! Accepts either a raw Anthropic export (bare array of conversations) or a
//! file previously saved by this tool (wrapped with a format-version marker).
//! Only the bare-array shape has never been deduplicated, so only that shape
//! runs dedup — a wrapped file is trusted as already-processed.
//!
//! Port of `unwrapUploadedJSON` at
//! [timeline.html:64953-64961](../../../timeline.html#L64953).

use std::fmt;

use crate::dedup::dedup_conversations;
use crate::model::Conversation;

#[derive(Debug)]
pub enum FormatError {
    /// The input wasn't even valid JSON.
    InvalidJson(serde_json::Error),
    /// Valid JSON, but neither a bare array nor a `{"conversations": [...]}"`
    /// object — matches the JS error message exactly.
    UnrecognizedShape,
    /// Matched one of the two recognized shapes, but a conversation or
    /// message inside it didn't match the expected schema.
    InvalidConversation(serde_json::Error),
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::InvalidJson(e) => write!(f, "invalid JSON: {e}"),
            FormatError::UnrecognizedShape => write!(
                f,
                "Expected either a bare array of conversations or a {{conversations: [...]}} object."
            ),
            FormatError::InvalidConversation(e) => write!(f, "invalid conversation data: {e}"),
        }
    }
}

impl std::error::Error for FormatError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FormatError::InvalidJson(e) => Some(e),
            FormatError::InvalidConversation(e) => Some(e),
            FormatError::UnrecognizedShape => None,
        }
    }
}

#[derive(Debug)]
pub struct UnwrapResult {
    pub conversations: Vec<Conversation>,
    /// `true` when the input arrived pre-wrapped (already deduplicated by an
    /// earlier pass); `false` when it was a fresh, bare-array export that this
    /// call just deduplicated.
    pub already_processed: bool,
}

/// Parses `raw` and unwraps it. See module docs for the two accepted shapes.
pub fn unwrap_uploaded_json(raw: &str) -> Result<UnwrapResult, FormatError> {
    let parsed: serde_json::Value = serde_json::from_str(raw).map_err(FormatError::InvalidJson)?;
    unwrap_uploaded_value(parsed)
}

/// Same as [`unwrap_uploaded_json`], for callers that already have a parsed
/// [`serde_json::Value`] (e.g. after validating it some other way first).
pub fn unwrap_uploaded_value(parsed: serde_json::Value) -> Result<UnwrapResult, FormatError> {
    if parsed.is_array() {
        let mut conversations: Vec<Conversation> =
            serde_json::from_value(parsed).map_err(FormatError::InvalidConversation)?;
        dedup_conversations(&mut conversations);
        return Ok(UnwrapResult {
            conversations,
            already_processed: false,
        });
    }
    if let Some(obj) = parsed.as_object() {
        if let Some(convs) = obj.get("conversations").filter(|v| v.is_array()) {
            let conversations: Vec<Conversation> =
                serde_json::from_value(convs.clone()).map_err(FormatError::InvalidConversation)?;
            return Ok(UnwrapResult {
                conversations,
                already_processed: true,
            });
        }
    }
    Err(FormatError::UnrecognizedShape)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

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

    #[test]
    fn unrecognized_shape_error_has_no_source() {
        let err = unwrap_uploaded_json("42").unwrap_err();
        assert!(matches!(err, FormatError::UnrecognizedShape));
        assert!(err.source().is_none());
    }

    #[test]
    fn scalar_json_is_unrecognized_shape() {
        let err = unwrap_uploaded_json("42").unwrap_err();
        assert!(matches!(err, FormatError::UnrecognizedShape));
    }

    #[test]
    fn conversations_field_not_an_array_is_unrecognized_shape() {
        let err = unwrap_uploaded_json(r#"{"conversations": "not an array"}"#).unwrap_err();
        assert!(matches!(err, FormatError::UnrecognizedShape));
    }

    #[test]
    fn malformed_conversation_inside_a_bare_array_is_a_distinct_surfaced_error() {
        let err = unwrap_uploaded_json(r#"[{"name": "missing uuid and sender"}]"#).unwrap_err();
        assert!(matches!(err, FormatError::InvalidConversation(_)));
    }
}
