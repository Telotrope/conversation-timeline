//! `GET`/`PATCH .../conversations/{conversation_id}/messages/{message_id}/flags`.
//! The two handlers below are given *different* state types
//! (`Arc<dyn MessageFlagsReader>` vs `Arc<dyn UserFlagWriter>`) -- that's
//! the concrete, checkable form of the auto/user separation from the
//! migration plan section 4.1: the PATCH handler's own source code has no
//! `AutoFlagWriter` in scope at all, because it's never one of its
//! parameters.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::message_flags::{
    FlagOverrides, MessageFlagRecord, MessageFlagsReader, UserFlagWriter,
};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::flag_handles::{FlagHandle, FlagHandleKey};

/// The body of a flag save. Stricter than `FlagOverrides`, which upload
/// processing also reads from uploaded files: an unknown field name is
/// refused rather than silently ignored, so a typo like `"cap"` gets a 400
/// naming it instead of turning into an empty save. See the migration
/// plan's §V2c.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlagPatchRequest {
    /// From `GET /export`'s `flag_handles`; proves the message is real.
    pub handle: FlagHandle,
    pub caps: Option<bool>,
    pub critical: Option<bool>,
    pub angry: Option<bool>,
}

pub async fn get_flags(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(reader): State<Arc<dyn MessageFlagsReader>>,
    Path((conversation_id, message_id)): Path<(ConversationId, MessageId)>,
) -> Result<Json<MessageFlagRecord>, ApiError> {
    let record = reader
        .get(&user_id, conversation_id, message_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(record))
}

pub async fn patch_flags(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(writer): State<Arc<dyn UserFlagWriter>>,
    State(key): State<Arc<FlagHandleKey>>,
    Path((conversation_id, message_id)): Path<(ConversationId, MessageId)>,
    body: Result<Json<FlagPatchRequest>, JsonRejection>,
) -> Result<Json<MessageFlagRecord>, ApiError> {
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
    if !key.verify(&user_id, conversation_id, message_id, &request.handle) {
        return Err(ApiError::Forbidden(
            "flag handle does not match this message; reload the page's data".to_string(),
        ));
    }
    let record = writer
        .set_user_flags(&user_id, conversation_id, message_id, overrides)
        .await?;
    Ok(Json(record))
}
