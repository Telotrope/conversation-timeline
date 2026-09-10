//! In-memory `UploadStore`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::model::ConversationId;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadRecord, UploadStatus, UploadStore};

#[derive(Default)]
pub struct InMemoryUploadStore {
    records: Mutex<HashMap<(UserId, UploadId), UploadRecord>>,
}

impl InMemoryUploadStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UploadStore for InMemoryUploadStore {
    async fn create_pending(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        raw_object_key: &str,
    ) -> Result<(), StoreError> {
        let record = UploadRecord {
            upload_id,
            status: UploadStatus::Pending,
            raw_object_key: raw_object_key.to_string(),
        };
        self.records
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert((user_id.clone(), upload_id), record);
        Ok(())
    }

    async fn get(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadRecord>, StoreError> {
        Ok(self
            .records
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), upload_id))
            .cloned())
    }

    async fn mark_processing(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<(), StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records
            .get_mut(&(user_id.clone(), upload_id))
            .ok_or(StoreError::NotFound)?;
        record.status = UploadStatus::Processing;
        Ok(())
    }

    async fn mark_ready(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        conversation_ids: Vec<ConversationId>,
    ) -> Result<(), StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records
            .get_mut(&(user_id.clone(), upload_id))
            .ok_or(StoreError::NotFound)?;
        record.status = UploadStatus::Ready { conversation_ids };
        Ok(())
    }

    async fn mark_failed(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        reason: String,
    ) -> Result<(), StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records
            .get_mut(&(user_id.clone(), upload_id))
            .ok_or(StoreError::NotFound)?;
        record.status = UploadStatus::Failed { reason };
        Ok(())
    }
}
