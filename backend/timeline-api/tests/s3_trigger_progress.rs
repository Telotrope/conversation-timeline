//! The S3-triggered path counts each processing attempt and records a failed
//! attempt's error, so the page can show a retry (plan
//! `2026-10-02-upload-processing-failures.md` §3). Runs `handle_s3_event`,
//! what the Lambda runs, against the in-memory stores.

use std::sync::Arc;

use async_trait::async_trait;
use aws_lambda_events::event::s3::S3Event;
use serde_json::{json, Value};
use timeline_api::s3_trigger::{handle_s3_event, ProcessingStores, RecordError};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{
    raw_object_key, UploadOutcome, UploadOutcomeStore, UploadProgress,
};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");
const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");

fn event_for(key: &str) -> S3Event {
    let mut event: Value = serde_json::from_str(SAMPLE).unwrap();
    event["Records"][0]["s3"]["object"]["key"] = json!(key);
    serde_json::from_value(event).unwrap()
}

fn stores_with(upload_outcome_store: Arc<dyn UploadOutcomeStore>) -> ProcessingStores {
    ProcessingStores {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_outcome_store,
        conversation_summary_store: Arc::new(InMemoryConversationSummaryStore::new()),
        user_flag_writer: Arc::new(InMemoryMessageFlagsStore::new()),
    }
}

fn ids() -> (UserId, UploadId) {
    (
        UserId("alice".to_string()),
        UploadId(uuid::Uuid::from_u128(9)),
    )
}

#[tokio::test]
async fn a_successful_attempt_is_counted_once_and_ends_ready() {
    let stores = stores_with(Arc::new(InMemoryUploadOutcomeStore::new()));
    let (user, upload) = ids();
    let key = raw_object_key(&user, upload);
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();

    handle_s3_event(event_for(&key), &stores).await.unwrap();

    let progress = stores
        .upload_outcome_store
        .get_progress(&user, upload)
        .await
        .unwrap();
    assert_eq!(
        progress,
        Some(UploadProgress {
            attempts: 1,
            last_error: None
        })
    );
    let outcome = stores
        .upload_outcome_store
        .get_outcome(&user, upload)
        .await
        .unwrap();
    assert!(
        matches!(outcome, Some(UploadOutcome::Ready { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn each_failed_attempt_is_counted_and_its_error_kept_for_the_page() {
    // No bytes stored: every attempt fails reading the file, a storage error
    // Lambda retries.
    let stores = stores_with(Arc::new(InMemoryUploadOutcomeStore::new()));
    let (user, upload) = ids();
    let key = raw_object_key(&user, upload);

    for attempt in 1..=2 {
        let err = handle_s3_event(event_for(&key), &stores).await.unwrap_err();
        assert!(
            matches!(&err.0[0], RecordError::Processing { .. }),
            "{err:?}"
        );
        let progress = stores
            .upload_outcome_store
            .get_progress(&user, upload)
            .await
            .unwrap();
        assert_eq!(
            progress,
            Some(UploadProgress {
                attempts: attempt,
                last_error: Some("object not found".to_string())
            })
        );
    }
    // Retrying is AWS's job; nothing here decides the upload has failed.
    assert_eq!(
        stores
            .upload_outcome_store
            .get_outcome(&user, upload)
            .await
            .unwrap(),
        None
    );
}

/// An outcome store whose progress writes fail: `record_attempt` when
/// `fail_count` is set, otherwise only `record_attempt_error`.
struct ProgressWritesFail {
    inner: InMemoryUploadOutcomeStore,
    fail_count: bool,
}

#[async_trait]
impl UploadOutcomeStore for ProgressWritesFail {
    async fn record_outcome(
        &self,
        u: &UserId,
        id: UploadId,
        o: UploadOutcome,
    ) -> Result<(), StoreError> {
        self.inner.record_outcome(u, id, o).await
    }
    async fn get_outcome(
        &self,
        u: &UserId,
        id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError> {
        self.inner.get_outcome(u, id).await
    }
    async fn record_attempt(&self, u: &UserId, id: UploadId) -> Result<usize, StoreError> {
        if self.fail_count {
            return Err(StoreError::NotFound);
        }
        self.inner.record_attempt(u, id).await
    }
    async fn record_attempt_error(
        &self,
        _: &UserId,
        _: UploadId,
        _: String,
    ) -> Result<(), StoreError> {
        Err(StoreError::NotFound)
    }
    async fn get_progress(
        &self,
        u: &UserId,
        id: UploadId,
    ) -> Result<Option<UploadProgress>, StoreError> {
        self.inner.get_progress(u, id).await
    }
}

#[tokio::test]
async fn failing_to_count_an_attempt_is_a_retried_error_and_nothing_is_processed() {
    let store = ProgressWritesFail {
        inner: InMemoryUploadOutcomeStore::new(),
        fail_count: true,
    };
    let stores = stores_with(Arc::new(store));
    let (user, upload) = ids();
    let key = raw_object_key(&user, upload);
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();

    let err = handle_s3_event(event_for(&key), &stores).await.unwrap_err();

    assert_eq!(
        err.0[0].to_string(),
        format!("processing {key:?} failed: item not found")
    );
    assert_eq!(
        stores
            .upload_outcome_store
            .get_outcome(&user, upload)
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn failing_to_record_an_attempts_error_still_returns_the_original_error() {
    let store = ProgressWritesFail {
        inner: InMemoryUploadOutcomeStore::new(),
        fail_count: false,
    };
    let stores = stores_with(Arc::new(store));
    let (user, upload) = ids();
    let key = raw_object_key(&user, upload);

    let err = handle_s3_event(event_for(&key), &stores).await.unwrap_err();

    assert_eq!(
        err.0[0].to_string(),
        format!("processing {key:?} failed: object not found")
    );
}
