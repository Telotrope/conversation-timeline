//! In-memory `UploadOutcomeStore`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};

#[derive(Default)]
pub struct InMemoryUploadOutcomeStore {
    outcomes: Mutex<HashMap<(UserId, UploadId), UploadOutcome>>,
}

impl InMemoryUploadOutcomeStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UploadOutcomeStore for InMemoryUploadOutcomeStore {
    async fn record_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        outcome: UploadOutcome,
    ) -> Result<(), StoreError> {
        self.outcomes
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert((user_id.clone(), upload_id), outcome);
        Ok(())
    }

    async fn get_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError> {
        Ok(self
            .outcomes
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), upload_id))
            .cloned())
    }
}
