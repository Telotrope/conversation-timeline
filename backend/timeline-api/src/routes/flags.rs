//! `GET`/`PATCH .../conversations/{conversation_id}/messages/{message_id}/flags`.
//! The PATCH handler is given a `UserFlagWriter` and no `AutoFlagWriter` --
//! the concrete, checkable form of the automatic/yours separation from the
//! migration plan section 4.1: its source has no way to write an automatic
//! flag, because no such capability is among its parameters.
//!
//! Flags live on the message rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3). A save
//! names its message by conversation and id, as before; the server finds
//! the message among the conversation's rows, writes your flags, then
//! recounts the message's session in the same request (§6) and raises the
//! user's data version. It is not split into parts: splitting would leave
//! the session's counts half-updated between them (§8c).

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use timeline_core::flag_values::{FlagOverrides, FlagSet};
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{MessageReader, UserFlagWriter};
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_message::{Entry, StoredMessage};
use timeline_core::stored_session::{Placement, StoredSession};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::flag_handles::{FlagHandle, FlagHandleKey};
use crate::message_query::WalkStores;
use crate::request_record::note;
use crate::session_recount::recount_session;

/// The body of a flag save. Stricter than `FlagOverrides`, which upload
/// processing also reads from uploaded files: an unknown field name is
/// refused rather than silently ignored, so a typo like `"cap"` gets a 400
/// naming it instead of turning into an empty save. See the migration
/// plan's §V2c.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagPatchRequest {
    /// From `GET /messages`' rows; proves the message is real.
    pub handle: FlagHandle,
    pub caps: Option<bool>,
    pub critical: Option<bool>,
    pub angry: Option<bool>,
}

/// A message's flags. `auto` is all false until the scan has run on it.
#[derive(Debug, Serialize)]
pub struct FlagsReply {
    pub message_id: MessageId,
    pub auto: FlagSet,
    /// Whether the scan has looked at the message.
    pub scanned: bool,
    pub user: FlagOverrides,
}

/// A save's answer: the message's flags, its session's new counts, and the
/// data version the save raised.
#[derive(Debug, Serialize)]
pub struct SaveReply {
    #[serde(flatten)]
    pub flags: FlagsReply,
    pub session: StoredSession,
    pub data_version: u64,
}

/// Your message `message_id` in `conversation_id`, or 404.
async fn your_message(
    reader: &dyn MessageReader,
    user_id: &UserId,
    conversation_id: ConversationId,
    message_id: MessageId,
) -> Result<StoredMessage, ApiError> {
    match reader
        .find_entry(user_id, conversation_id, message_id)
        .await?
    {
        Some(Entry::Message(m)) if m.sender == Sender::Human => Ok(m),
        _ => Err(ApiError::NotFound),
    }
}

fn reply_for(message: &StoredMessage) -> FlagsReply {
    let flags = message.flags.unwrap_or_default();
    FlagsReply {
        message_id: message.key.id,
        auto: flags.auto.unwrap_or_default(),
        scanned: flags.auto.is_some(),
        user: flags.user,
    }
}

pub async fn get_flags(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(reader): State<Arc<dyn MessageReader>>,
    Path((conversation_id, message_id)): Path<(ConversationId, MessageId)>,
) -> Result<Json<FlagsReply>, ApiError> {
    let message = your_message(reader.as_ref(), &user_id, conversation_id, message_id).await?;
    Ok(Json(reply_for(&message)))
}

/// The session holding `message`: the conversation's only session when it
/// is placed by its span, otherwise the one whose span contains the time.
fn session_of(
    sessions: Vec<StoredSession>,
    message: &StoredMessage,
) -> Result<StoredSession, ApiError> {
    sessions
        .into_iter()
        .find(|s| s.placement == Placement::Span || s.contains(message.key.at))
        // Unreachable backstop: processing cuts every message into a
        // session, so a stored message always has one.
        .ok_or_else(|| {
            ApiError::Internal(format!(
                "message {} has no session in conversation {}",
                message.key.id, message.key.conversation_id
            ))
        })
}

#[allow(clippy::too_many_arguments)]
pub async fn patch_flags(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(reader): State<Arc<dyn MessageReader>>,
    State(writer): State<Arc<dyn UserFlagWriter>>,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(key): State<Arc<FlagHandleKey>>,
    Path((conversation_id, message_id)): Path<(ConversationId, MessageId)>,
    body: Result<Json<FlagPatchRequest>, JsonRejection>,
) -> Result<Json<SaveReply>, ApiError> {
    // axum would answer a malformed body with its own 422; this route
    // answers 400 with serde's explanation (e.g. "unknown field `cap`").
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let overrides = FlagOverrides {
        caps: request.caps,
        critical: request.critical,
        angry: request.angry,
    };
    if overrides == FlagOverrides::default() {
        return Err(ApiError::BadRequest(
            "nothing to save: give at least one of caps, critical, angry as true or false"
                .to_string(),
        ));
    }
    // For this request's log line (crate::request_log).
    note("conversation_id", conversation_id.to_string());
    note("message_id", message_id.to_string());
    note("caps", request.caps);
    note("critical", request.critical);
    note("angry", request.angry);
    if !key.verify(&user_id, conversation_id, message_id, &request.handle) {
        note("handle", "refused");
        return Err(ApiError::Forbidden(
            "flag handle does not match this message; reload the page's data".to_string(),
        ));
    }
    note("handle", "accepted");
    let mut message = your_message(reader.as_ref(), &user_id, conversation_id, message_id).await?;
    message.flags = Some(
        writer
            .set_user_flags(&user_id, message.key, overrides)
            .await?,
    );
    let session = session_of(
        sessions.sessions_of(&user_id, conversation_id).await?,
        &message,
    )?;
    let stores = WalkStores {
        sessions: sessions.as_ref(),
        messages: reader.as_ref(),
    };
    let session = recount_session(stores, sessions.as_ref(), &user_id, &session).await?;
    let record = user_records.raise_version(&user_id).await?;
    Ok(Json(SaveReply {
        flags: reply_for(&message),
        session,
        data_version: record.data_version,
    }))
}
