//! Rebuilds a user's conversations from storage: each from the first file
//! it came in, plus the messages later files added to it (plan
//! `docs/plans/2026-10-05-screen-flow.md` §8b-2). The one way to read a
//! stored conversation back, used by both the export (`GET /export`) and
//! the scan (`POST /detect`), so a message a later file added is exported
//! and scanned like any other (plan C23).
//!
//! Each first file is parsed once, however many of the requested
//! conversations came from it. Later files are never re-read in full: only
//! the small stored lists of the messages they added.

use std::collections::HashMap;

use timeline_core::model::{ChatMessage, Conversation, ConversationId};
use timeline_core::ports::conversations::ConversationSummary;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{addition_object_key, raw_object_key};
use timeline_core::{dedup_chat_messages, unwrap_uploaded_json};

use crate::error::ApiError;

fn integrity_error(context: &str, e: impl std::fmt::Display) -> ApiError {
    // Everything read here was written by our own processing, which parsed
    // it first, so a failure now means stored data stopped being good -- an
    // internal fault, not a problem with the request.
    ApiError::Internal(format!("{context}: {e}"))
}

/// The conversations `summaries` describe, rebuilt, in the same order.
pub async fn rebuild_conversations(
    object_store: &dyn ObjectStore,
    user_id: &UserId,
    summaries: &[ConversationSummary],
) -> Result<Vec<Conversation>, ApiError> {
    let mut first_files: Vec<UploadId> = summaries.iter().map(|s| s.source.upload_id).collect();
    first_files.sort_by_key(|id| id.0);
    first_files.dedup();

    // Keyed by file as well as conversation: a later file that brought new
    // conversations is some conversations' first file too, and its copy of
    // an older conversation must not replace that conversation's own.
    let mut from_first_file: HashMap<(UploadId, ConversationId), Conversation> = HashMap::new();
    for upload_id in first_files {
        let raw_bytes = object_store
            .get(&raw_object_key(user_id, upload_id))
            .await?;
        let raw_text = String::from_utf8(raw_bytes)
            .map_err(|e| integrity_error("stored upload was not valid UTF-8", e))?;
        let parsed = unwrap_uploaded_json(&raw_text)
            .map_err(|e| integrity_error("stored upload no longer parses", e))?;
        for conversation in parsed.conversations {
            from_first_file.insert((upload_id, conversation.uuid), conversation);
        }
    }

    let mut rebuilt = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let mut conversation = from_first_file
            .remove(&(summary.source.upload_id, summary.conversation_id))
            .ok_or_else(|| {
                integrity_error(
                    "conversation summary has no matching parsed conversation",
                    summary.conversation_id,
                )
            })?;
        if !summary.additions.is_empty() {
            for upload_id in &summary.additions {
                let key = addition_object_key(user_id, summary.conversation_id, *upload_id);
                let added: Vec<ChatMessage> =
                    serde_json::from_slice(&object_store.get(&key).await?)
                        .map_err(|e| integrity_error("stored added messages no longer parse", e))?;
                conversation.chat_messages.extend(added);
            }
            conversation.chat_messages.sort_by_key(|m| m.created_at);
            conversation.chat_messages = dedup_chat_messages(&conversation.chat_messages);
        }
        rebuilt.push(conversation);
    }
    Ok(rebuilt)
}
