//! `GET /export` -- regenerates the annotated `conversations.json` export:
//! the original uploaded content, plus every human message's stored
//! auto/user flags embedded the same way the original tool embedded them
//! ([timeline.html:64980-64994](../../../timeline.html#L64980):
//! `_claude_timeline_auto`/`_claude_timeline_user`), so the result can be
//! re-uploaded and recognized as already-processed. Written to the object
//! store and served back via presigned GET, per the migration plan's §1
//! "Annotated export" row.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use serde_json::json;
use timeline_core::model::{Conversation, ConversationId};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::MessageFlagsReader;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::raw_object_key;
use timeline_core::{unwrap_uploaded_json, Sender};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;

/// Long enough for a slow download of a large export; see
/// `routes::uploads::UPLOAD_URL_TTL` for the matching upload-side constant.
const EXPORT_URL_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize)]
pub struct ExportResponse {
    pub export_url: String,
}

fn integrity_error(context: &str, e: impl std::fmt::Display) -> ApiError {
    // Every conversation reachable here already parsed successfully once,
    // at upload time -- reaching this branch means previously-good data
    // stopped being good, which is a genuine "something's wrong on our
    // side" condition, not a normal user-facing failure.
    ApiError::Internal(format!("{context}: {e}"))
}

pub async fn export(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(object_store): State<Arc<dyn ObjectStore>>,
    State(conversation_summary_store): State<Arc<dyn ConversationSummaryStore>>,
    State(flags_reader): State<Arc<dyn MessageFlagsReader>>,
) -> Result<Json<ExportResponse>, ApiError> {
    let summaries = conversation_summary_store.list_for_user(&user_id).await?;

    let mut upload_ids: Vec<_> = summaries.iter().map(|s| s.upload_id).collect();
    upload_ids.sort_by_key(|id| id.0);
    upload_ids.dedup();

    // Re-parse each distinct upload's raw bytes once, even if it produced
    // several conversations, rather than refetching per conversation. The
    // raw object's key is recomputed rather than read back from storage --
    // see `raw_object_key`'s doc comment and the migration plan's
    // §V2a-revision.
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

    let mut annotated = Vec::with_capacity(summaries.len());
    for summary in &summaries {
        let mut conversation = conversations_by_id
            .remove(&summary.conversation_id)
            .ok_or_else(|| {
                integrity_error(
                    "conversation summary has no matching parsed conversation",
                    summary.conversation_id,
                )
            })?;
        for message in &mut conversation.chat_messages {
            if message.sender != Sender::Human {
                continue;
            }
            if let Some(record) = flags_reader
                .get(&user_id, summary.conversation_id, message.uuid)
                .await?
            {
                message.extra.insert(
                    "_claude_timeline_auto".to_string(),
                    json!({
                        "caps": record.auto.caps,
                        "critical": record.auto.critical,
                        "angry": record.auto.angry,
                        "source": "heuristic",
                    }),
                );
                message
                    .extra
                    .insert("_claude_timeline_user".to_string(), json!(record.user));
            }
        }
        annotated.push(conversation);
    }

    let export_bytes = serde_json::to_vec(&json!({ "conversations": annotated }))
        .map_err(|e| integrity_error("failed to serialize the export payload", e))?;
    let export_key = format!("export/{user_id}/{}.json", uuid::Uuid::new_v4());
    object_store.put(&export_key, export_bytes).await?;
    let export_url = object_store.presign_get(&export_key, EXPORT_URL_TTL).await?;

    Ok(Json(ExportResponse { export_url }))
}
