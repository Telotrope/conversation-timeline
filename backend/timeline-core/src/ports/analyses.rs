//! Saved results of the two server analyses (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c, §8c), one
//! row per analysis and options, holding the numbers so far, the data
//! version they were counted from and, while unfinished, where to carry on.

use async_trait::async_trait;

use super::errors::StoreError;
use super::ids::UserId;
use crate::server_analyses::{AnalysisKey, SavedAnalysis};

#[async_trait]
pub trait AnalysisStore: Send + Sync {
    async fn get(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
    ) -> Result<Option<SavedAnalysis>, StoreError>;

    /// Replaces the saved analysis under `key`.
    async fn put(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
        saved: &SavedAnalysis,
    ) -> Result<(), StoreError>;
}
