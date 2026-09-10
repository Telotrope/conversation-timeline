//! `GET`/`PATCH .../conversations/{conversation_id}/messages/{message_id}/flags`.
//! The two handlers below are given *different* state types
//! (`Arc<dyn MessageFlagsReader>` vs `Arc<dyn UserFlagWriter>`) -- that's
//! the concrete, checkable form of the auto/user separation from the
//! migration plan section 4.1: the PATCH handler's own source code has no
//! `AutoFlagWriter` in scope at all, because it's never one of its
//! parameters.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::message_flags::{
    FlagOverrides, MessageFlagRecord, MessageFlagsReader, UserFlagWriter,
};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;

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
    Path((conversation_id, message_id)): Path<(ConversationId, MessageId)>,
    Json(overrides): Json<FlagOverrides>,
) -> Result<Json<MessageFlagRecord>, ApiError> {
    let record = writer
        .set_user_flags(&user_id, conversation_id, message_id, overrides)
        .await?;
    Ok(Json(record))
}
