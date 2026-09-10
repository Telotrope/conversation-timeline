//! Tracks one upload's lifecycle (pending -> processing -> ready/failed).
//! Backed by the `Conversations` DynamoDB table from the migration plan
//! §1.3/§1.6 — an upload's own status row and the per-conversation summary
//! rows it eventually produces share that table, distinguished by sort key
//! in the concrete adapter, not by a separate table this crate doesn't list.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::errors::StoreError;
use super::ids::{UploadId, UserId};
use crate::model::ConversationId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UploadStatus {
    Pending,
    Processing,
    Ready {
        conversation_ids: Vec<ConversationId>,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadRecord {
    pub upload_id: UploadId,
    pub status: UploadStatus,
    /// The S3 key the raw upload was (or will be) written to.
    pub raw_object_key: String,
}

#[async_trait]
pub trait UploadStore: Send + Sync {
    /// Creates the initial `Pending` record, before the client has even
    /// uploaded the object — called from the `POST /uploads` handler at the
    /// same time it issues the presigned PUT URL.
    async fn create_pending(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        raw_object_key: &str,
    ) -> Result<(), StoreError>;

    async fn get(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadRecord>, StoreError>;

    async fn mark_processing(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<(), StoreError>;

    async fn mark_ready(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        conversation_ids: Vec<ConversationId>,
    ) -> Result<(), StoreError>;

    async fn mark_failed(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        reason: String,
    ) -> Result<(), StoreError>;
}
