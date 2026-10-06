//! The `FailProcessing` test setting (plan
//! `2026-10-02-upload-processing-failures.md` §2b): when on, every
//! processing attempt is counted, recorded as failed with a plain reason,
//! and returned as an error for AWS to retry, without reading the file.
//! Off by default; anything but `on` or `off` is refused.

use std::sync::Arc;

use aws_lambda_events::event::s3::S3Event;
use serde_json::{json, Value};
use timeline_api::aws_settings::{DeliberateFailure, EventLogging, InvalidDeliberateFailure};
use timeline_api::s3_trigger::{
    handle_raw_s3_event_with, handle_s3_event_with, ProcessingStores, RawEventError,
};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome, UploadProgress};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");
const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");
const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

fn setting(value: Option<&str>) -> Result<DeliberateFailure, InvalidDeliberateFailure> {
    DeliberateFailure::from_lookup(|name| {
        assert_eq!(name, "TIMELINE_FAIL_PROCESSING");
        value.map(str::to_string)
    })
}

#[test]
fn the_setting_is_off_when_missing_and_takes_only_on_or_off() {
    assert_eq!(setting(None), Ok(DeliberateFailure::Off));
    assert_eq!(setting(Some("off")), Ok(DeliberateFailure::Off));
    assert_eq!(setting(Some("on")), Ok(DeliberateFailure::On));
    for bad in ["ON", "yes", "true", ""] {
        let err = setting(Some(bad)).unwrap_err();
        assert_eq!(err, InvalidDeliberateFailure(bad.to_string()));
        assert_eq!(
            err.to_string(),
            format!("TIMELINE_FAIL_PROCESSING must be \"on\" or \"off\", not {bad:?}")
        );
    }
}

fn raw_event(key: &str) -> Value {
    let mut event: Value = serde_json::from_str(SAMPLE).unwrap();
    event["Records"][0]["s3"]["object"]["key"] = json!(key);
    event
}

async fn stored_upload() -> (ProcessingStores, UserId, UploadId, String) {
    let stores = ProcessingStores {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        conversation_summary_store: Arc::new(InMemoryConversationSummaryStore::new()),
        user_flag_writer: Arc::new(InMemoryMessageFlagsStore::new()),
    };
    let (user, upload) = (
        UserId("alice".to_string()),
        UploadId(uuid::Uuid::from_u128(4)),
    );
    let key = raw_object_key(&user, upload);
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();
    record_upload_facts(stores.upload_outcome_store.as_ref(), &key).await;
    (stores, user, upload, key)
}

#[tokio::test]
async fn when_on_each_attempt_is_counted_and_fails_on_purpose_without_processing() {
    let (stores, user, upload, key) = stored_upload().await;

    for attempt in 1..=2 {
        let err = handle_raw_s3_event_with(
            raw_event(&key),
            &stores,
            EventLogging::Off,
            DeliberateFailure::On,
            |_| {},
        )
        .await
        .unwrap_err();
        let RawEventError::Trigger(trigger) = err else {
            panic!("expected a record failure")
        };
        assert_eq!(
            trigger.0[0].to_string(),
            format!("processing {key:?} failed: failing on purpose (FailProcessing is on)")
        );
        assert_eq!(
            stores
                .upload_outcome_store
                .get_progress(&user, upload)
                .await
                .unwrap(),
            Some(UploadProgress {
                attempts: attempt,
                last_error: Some("failing on purpose (FailProcessing is on)".to_string()),
                processing: None,
            })
        );
    }
    // Retried, not decided: no outcome, and the file was never processed.
    assert_eq!(
        stores
            .upload_outcome_store
            .get_outcome(&user, upload)
            .await
            .unwrap(),
        None
    );
    assert!(stores
        .conversation_summary_store
        .list_for_user(&user)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn when_off_the_upload_is_processed_as_usual() {
    let (stores, user, upload, key) = stored_upload().await;
    let event: S3Event = serde_json::from_value(raw_event(&key)).unwrap();

    handle_s3_event_with(event, &stores, DeliberateFailure::Off)
        .await
        .unwrap();

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

#[test]
fn the_template_switch_is_off_by_default_and_only_the_processing_function_reads_it() {
    let parameter = TEMPLATE
        .split("\n  FailProcessing:\n")
        .nth(1)
        .expect("template declares FailProcessing");
    assert!(parameter.contains("Default: \"off\""));
    assert!(parameter.contains("AllowedValues: [\"off\", \"on\"]"));
    let setting = "TIMELINE_FAIL_PROCESSING: !Ref FailProcessing";
    assert_eq!(TEMPLATE.matches(setting).count(), 1);
    let processing = TEMPLATE
        .split("\n  ProcessUploadFunction:\n")
        .nth(1)
        .unwrap()
        .split("\n  RecordFailedUploadFunction:\n")
        .next()
        .unwrap();
    assert!(processing.contains(setting));
}

/// Records what `POST /uploads` would have recorded, for a file put
/// straight into storage: processing needs the file's name, upload time and
/// human name (plan 2026-10-05-screen-flow.md §8b).
async fn record_upload_facts(
    store: &dyn timeline_core::ports::uploads::UploadOutcomeStore,
    key: &str,
) {
    let (user, upload) = timeline_core::ports::uploads::parse_raw_object_key(key).unwrap();
    store
        .record_received(
            &user,
            upload,
            timeline_core::conversation_metadata::UploadFacts {
                file_name: timeline_core::labels::FileName::parse("conversations.json").unwrap(),
                uploaded_at: chrono::Utc::now(),
                file_written_at: None,
                human_name: timeline_core::labels::PersonName::parse("Alice").unwrap(),
            },
        )
        .await
        .unwrap();
}
