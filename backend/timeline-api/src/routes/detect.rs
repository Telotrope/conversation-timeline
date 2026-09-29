//! `POST /detect` -- runs the non-generative (keyword/dictionary/sentiment)
//! flag detection over a slice of the caller's conversations.
//!
//! Detection deliberately does **not** happen at upload time. It is work
//! proportional to the number of speech acts in the export, and nothing here
//! has measured how long that takes, so it runs only when the user asks for
//! it and reports progress while it does -- see
//! `docs/plans/2026-09-28-frontend-quality-of-life.md` Phase 4.
//!
//! **Why this is paged rather than one call.** A single request that returns
//! when the whole pass is finished can report no progress at all. Paging lets
//! the client drive the loop and advance a real, determinate progress bar --
//! the same shape as the page's existing AI classification pass. The page
//! size is the caller's choice; the server only promises that a given
//! `(offset, limit)` window is stable for the duration of the loop.
//!
//! That stability is why this route sorts by conversation id before slicing.
//! `ConversationSummaryStore::list_for_user` returns summaries in whatever
//! order its backing store yields -- for the in-memory adapter that is
//! `HashMap` iteration order, which is randomized per process. Paging over an
//! arbitrarily ordered list would be free to skip or repeat conversations, so
//! the order is pinned here rather than assumed.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use timeline_core::model::{Conversation, ConversationId};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::AutoFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::raw_object_key;
use timeline_core::{extract_text, unwrap_uploaded_json, Sender};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::processing::heuristic_flags;

/// Chosen so a caller that omits `limit` still gets a bounded unit of work
/// rather than the whole export in one unreportable block.
const DEFAULT_LIMIT: usize = 5;

#[derive(Deserialize, Default)]
pub struct DetectRequest {
    #[serde(default)]
    pub offset: usize,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct DetectResponse {
    /// Total conversations this user has, so the client can render progress
    /// as a proportion without a separate call.
    pub total_conversations: usize,
    /// How many conversations this call covered.
    pub conversations_processed: usize,
    /// How many human messages had flags computed and stored by this call.
    pub messages_detected: usize,
    /// The offset to send next, or `null` once the pass is complete.
    pub next_offset: Option<usize>,
}

fn integrity_error(context: &str, e: impl std::fmt::Display) -> ApiError {
    // Anything reachable here parsed successfully at upload time, so a
    // failure now means previously-good stored data stopped being good -- a
    // genuine internal fault, not a user-facing input problem.
    ApiError::Internal(format!("{context}: {e}"))
}

pub async fn detect(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(object_store): State<Arc<dyn ObjectStore>>,
    State(conversation_summary_store): State<Arc<dyn ConversationSummaryStore>>,
    State(auto_flag_writer): State<Arc<dyn AutoFlagWriter>>,
    Json(request): Json<DetectRequest>,
) -> Result<Json<DetectResponse>, ApiError> {
    let mut summaries = conversation_summary_store.list_for_user(&user_id).await?;
    // See the module doc: paging needs a stable order and the store does not
    // promise one.
    summaries.sort_by_key(|s| s.conversation_id.0);

    let total_conversations = summaries.len();
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).max(1);
    let window: Vec<_> = summaries
        .into_iter()
        .skip(request.offset)
        .take(limit)
        .collect();

    // Parse each distinct upload once, even when several of this window's
    // conversations came from the same one.
    let mut upload_ids: Vec<_> = window.iter().map(|s| s.upload_id).collect();
    upload_ids.sort_by_key(|id| id.0);
    upload_ids.dedup();

    let mut conversations_by_id: HashMap<ConversationId, Conversation> = HashMap::new();
    for upload_id in upload_ids {
        let key = raw_object_key(&user_id, upload_id);
        let raw_bytes = object_store.get(&key).await?;
        let raw_text = String::from_utf8(raw_bytes)
            .map_err(|e| integrity_error("stored upload was not valid UTF-8", e))?;
        let parsed = unwrap_uploaded_json(&raw_text)
            .map_err(|e| integrity_error("stored upload no longer parses", e))?;
        for conversation in parsed.conversations {
            conversations_by_id.insert(conversation.uuid, conversation);
        }
    }

    let conversations_processed = window.len();
    let mut messages_detected = 0;
    for summary in window {
        let conversation = conversations_by_id
            .remove(&summary.conversation_id)
            .ok_or_else(|| {
                integrity_error(
                    "conversation summary has no matching parsed conversation",
                    summary.conversation_id,
                )
            })?;
        for message in &conversation.chat_messages {
            if message.sender != Sender::Human {
                continue;
            }
            let flags = heuristic_flags(&extract_text(message));
            auto_flag_writer
                .set_auto_flags(&user_id, summary.conversation_id, message.uuid, flags)
                .await?;
            messages_detected += 1;
        }
    }

    let consumed = request.offset + conversations_processed;
    Ok(Json(DetectResponse {
        total_conversations,
        conversations_processed,
        messages_detected,
        next_offset: if consumed < total_conversations {
            Some(consumed)
        } else {
            None
        },
    }))
}
