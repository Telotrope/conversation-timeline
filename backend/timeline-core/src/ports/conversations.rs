//! Read access to conversation summaries — powers `GET /conversations`
//! without ever touching the raw S3 blob, per the migration plan §1.3.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::{UploadId, UserId};
use crate::model::{ConversationId, ConversationName};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSummary {
    pub conversation_id: ConversationId,
    pub upload_id: UploadId,
    pub name: ConversationName,
    pub message_count: usize,
}

#[async_trait]
pub trait ConversationStore: Send + Sync {
    async fn list_for_user(&self, user_id: &UserId)
        -> Result<Vec<ConversationSummary>, StoreError>;

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError>;
}
