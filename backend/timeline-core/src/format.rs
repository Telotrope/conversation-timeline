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
