//! In-memory `UserRecordStore` and `AnalysisStore`: the user's own record
//! and saved analyses, which on AWS share the conversations table.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use timeline_core::ports::analyses::AnalysisStore;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::user_record::{Totals, UserRecord, UserRecordStore};
use timeline_core::server_analyses::{AnalysisKey, SavedAnalysis};

#[derive(Default)]
pub struct InMemoryUserRecordStore {
    records: Mutex<HashMap<UserId, UserRecord>>,
    analyses: Mutex<HashMap<(UserId, AnalysisKey), SavedAnalysis>>,
}

impl InMemoryUserRecordStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl UserRecordStore for InMemoryUserRecordStore {
    async fn get(&self, user_id: &UserId) -> Result<UserRecord, StoreError> {
        Ok(self
            .records
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(user_id)
            .copied()
            .unwrap_or_default())
    }

    async fn record_change(
        &self,
        user_id: &UserId,
        change: Totals,
    ) -> Result<UserRecord, StoreError> {
        let mut records = self.records.lock().expect("in-memory store mutex poisoned");
        let record = records.entry(user_id.clone()).or_default();
        record.data_version += 1;
        record.totals = record.totals.plus(change);
        Ok(*record)
    }
}

#[async_trait]
impl AnalysisStore for InMemoryUserRecordStore {
    async fn get(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
    ) -> Result<Option<SavedAnalysis>, StoreError> {
        Ok(self
            .analyses
            .lock()
            .expect("in-memory store mutex poisoned")
            .get(&(user_id.clone(), key.clone()))
            .cloned())
    }

    async fn put(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
        saved: &SavedAnalysis,
    ) -> Result<(), StoreError> {
        self.analyses
            .lock()
            .expect("in-memory store mutex poisoned")
            .insert((user_id.clone(), key.clone()), saved.clone());
        Ok(())
    }
}

impl crate::memory::resettable::Resettable for InMemoryUserRecordStore {
    fn reset(&self) {
        self.records
            .lock()
            .expect("in-memory store mutex poisoned")
            .clear();
        self.analyses
            .lock()
            .expect("in-memory store mutex poisoned")
            .clear();
    }
}
