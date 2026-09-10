//! `GET /conversations`.

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use timeline_core::ports::conversations::{ConversationStore, ConversationSummary};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;

pub async fn list_conversations(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationStore>>,
) -> Result<Json<Vec<ConversationSummary>>, ApiError> {
    let summaries = store.list_for_user(&user_id).await?;
    Ok(Json(summaries))
}
