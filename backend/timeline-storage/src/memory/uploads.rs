//! In-memory `UploadOutcomeStore`.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore, UploadProgress};

#[derive(Default)]
pub struct InMemoryUploadOutcomeStore {
    outcomes: Mutex<HashMap<(UserId, UploadId), UploadOutcome>>,
    progress: Mutex<HashMap<(UserId, UploadId), UploadProgress>>,
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

    async fn record_attempt(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<usize, StoreError> {
        let mut progress = self
            .progress
            .lock()
            .expect("in-memory store mutex poisoned");
        let entry = progress
            .entry((user_id.clone(), upload_id))
            .or_insert(UploadProgress {
                attempts: 0,
                last_error: None,
            });
        entry.attempts += 1;
        Ok(entry.attempts)
    }

    async fn record_attempt_error(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        error: String,
    ) -> Result<(), StoreError> {
        let mut progress = self
            .progress
            .lock()
            .expect("in-memory store mutex poisoned");
        progress
            .entry((user_id.clone(), upload_id))
            .or_insert(UploadProgress {
                attempts: 0,
                last_error: None,
            })
            .last_error = Some(error);
        Ok(())
    }

    async fn get_progress(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadProgress>, StoreError> {
        Ok(self
            .progress
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), upload_id))
            .cloned())
    }
}

impl crate::memory::resettable::Resettable for InMemoryUploadOutcomeStore {
    fn reset(&self) {
        self.outcomes
            .lock()
            .expect("in-memory store mutex poisoned")
            .clear();
        self.progress
            .lock()
            .expect("in-memory store mutex poisoned")
            .clear();
    }
}
