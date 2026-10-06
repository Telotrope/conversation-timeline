//! `GET /files/{conversation_id}/{message_id}/{number}`: a short-lived
//! address to download one file kept from a conversation (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4).
//!
//! The file must be one the message's row names, with its text stored; any
//! other is answered 404, like a file of another user's, so the answer
//! reveals nothing. The page shows the file without running any code in it
//! (the user, Q3); this route only hands out the address.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::Json;
use serde::Serialize;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::messages::MessageReader;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::file_object_key;
use timeline_core::stored_message::{Entry, FileContents, FileRef};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;

/// Long enough to open the file, short enough not to be worth keeping.
const FILE_URL_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Serialize)]
pub struct FileAddress {
    pub url: String,
    #[serde(flatten)]
    pub file: FileRef,
}

pub async fn file_address(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(messages): State<Arc<dyn MessageReader>>,
    State(object_store): State<Arc<dyn ObjectStore>>,
    Path((conversation_id, message_id, number)): Path<(ConversationId, MessageId, usize)>,
) -> Result<Json<FileAddress>, ApiError> {
    let Some(Entry::Message(message)) = messages
        .find_entry(&user_id, conversation_id, message_id)
        .await?
    else {
        return Err(ApiError::NotFound);
    };
    let file = message
        .files()
        .find(|f| f.number == number && f.contents == FileContents::Stored)
        .cloned()
        .ok_or(ApiError::NotFound)?;
    let key = file_object_key(&user_id, conversation_id, message_id, number);
    let url = object_store.presign_get(&key, FILE_URL_TTL).await?;
    Ok(Json(FileAddress { url, file }))
}
