//! Cursors: where a reply in parts stopped (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8c).
//!
//! The page treats a cursor as opaque text and sends it back unchanged with
//! the same request to carry on. On the wire it is URL-safe base64 of a small
//! JSON object naming the request it belongs to. It crosses a trust
//! boundary (the page could send anything), so it is parsed once, here,
//! into a typed [`Cursor`]; text that isn't one, or is another request's,
//! is a 400 saying which. A cursor can only point into the signed-in user's
//! own rows: the user always comes from the sign-in, never from the cursor.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use timeline_core::model::ConversationId;
use timeline_core::ports::ids::UploadId;
use timeline_core::stored_session::SessionKey;
use timeline_core::walk_cursor::WalkCursor;

use crate::error::ApiError;

/// The longest cursor text accepted: far more than any this server writes.
const MAX_CURSOR_CHARS: usize = 2_000;

/// Where each kind of request stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cursor {
    /// `GET /conversations`: the last record sent.
    Conversations { after: ConversationId },
    /// `GET /sessions`: the last session sent.
    Sessions { after: SessionKey },
    /// `GET /uploads`: the last file listed.
    Uploads { after: UploadId },
    /// `PUT /uploads/{id}/metadata`: the last conversation changed.
    UploadEdit { after: ConversationId },
    /// `GET /messages`: where the walk stopped.
    Messages { walk: WalkCursor },
    /// `GET /conversations/{id}/files`.
    Files { walk: WalkCursor },
    /// `POST /detect`.
    Scan { walk: WalkCursor },
    /// `GET /export`: where the walk stopped, the conversation whose
    /// messages the last part left open, if any, and whether any
    /// conversation has been written yet (the next needs a comma before it).
    Export {
        walk: WalkCursor,
        open: Option<ConversationId>,
        any: bool,
    },
}

impl Cursor {
    /// The text the page is given.
    pub fn to_text(self) -> String {
        // Unreachable backstop: a cursor is plain data, which always
        // serializes.
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&self).expect("a cursor serializes"))
    }

    /// Parses `text` from a request; see the module doc.
    pub fn parse(text: &str) -> Result<Cursor, ApiError> {
        let refuse = |why: &str| ApiError::BadRequest(format!("cursor refused: {why}"));
        if text.len() > MAX_CURSOR_CHARS {
            return Err(refuse("it is far longer than any this server gives"));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(text.as_bytes())
            .map_err(|_| refuse("it is not one this server gave (not the expected encoding)"))?;
        serde_json::from_slice(&bytes)
            .map_err(|_| refuse("it is not one this server gave (not the expected content)"))
    }
}

/// A cursor of another request's kind.
pub fn wrong_kind(expected: &str) -> ApiError {
    ApiError::BadRequest(format!(
        "cursor refused: it belongs to another kind of request, not {expected}"
    ))
}

/// Parses an optional cursor for one kind of request, with `pick`
/// returning its contents when the kind is right.
pub fn parse_for<T>(
    text: Option<&str>,
    expected: &str,
    pick: impl Fn(Cursor) -> Option<T>,
) -> Result<Option<T>, ApiError> {
    match text {
        None => Ok(None),
        Some(text) => pick(Cursor::parse(text)?)
            .map(Some)
            .ok_or_else(|| wrong_kind(expected)),
    }
}
