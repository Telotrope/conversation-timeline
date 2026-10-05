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
use timeline_core::model::MessageId;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::MessageFlagsReader;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::Sender;

use crate::auth_extractor::AuthenticatedUser;
use crate::conversation_rebuild::rebuild_conversations;
use crate::error::ApiError;
use crate::flag_handles::{FlagHandle, FlagHandleKey};
use crate::request_record::note;

/// Long enough for a slow download of a large export; see
/// `routes::uploads::UPLOAD_URL_TTL` for the matching upload-side constant.
const EXPORT_URL_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize)]
pub struct ExportResponse {
    pub export_url: String,
    /// One handle per user message in the export, keyed by message id. The
    /// page sends a message's handle back with each flag save; see
    /// `crate::flag_handles`. Delivered here, in this reply, so the
    /// exported `conversations.json` itself is unchanged.
    pub flag_handles: HashMap<MessageId, FlagHandle>,
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
    State(flag_handle_key): State<Arc<FlagHandleKey>>,
) -> Result<Json<ExportResponse>, ApiError> {
    let summaries = conversation_summary_store.list_for_user(&user_id).await?;

    let conversations = rebuild_conversations(object_store.as_ref(), &user_id, &summaries).await?;

    let mut annotated = Vec::with_capacity(summaries.len());
    let mut flag_handles = HashMap::new();
    for (summary, mut conversation) in summaries.iter().zip(conversations) {
        for message in &mut conversation.chat_messages {
            if message.sender != Sender::Human {
                continue;
            }
            flag_handles.insert(
                message.uuid,
                flag_handle_key.handle_for(&user_id, summary.conversation_id, message.uuid),
            );
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

    let annotated_count = annotated.len();
    let export_bytes = serde_json::to_vec(&json!({ "conversations": annotated }))
        .map_err(|e| integrity_error("failed to serialize the export payload", e))?;
    // For this request's log line (crate::request_log).
    note("conversations", annotated_count);
    note("export_bytes", export_bytes.len());
    let export_key = format!("export/{user_id}/{}.json", uuid::Uuid::new_v4());
    object_store.put(&export_key, export_bytes).await?;
    let export_url = object_store
        .presign_get(&export_key, EXPORT_URL_TTL)
        .await?;

    Ok(Json(ExportResponse {
        export_url,
        flag_handles,
    }))
}
