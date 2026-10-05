//! Processing a file that meets conversations already stored, and a file no
//! `POST /uploads` recorded (plan docs/plans/2026-10-05-screen-flow.md
//! §8b-8b-2): run through the processing Lambda's own handler, so the run's
//! log line is checked too.

use std::cell::RefCell;
use std::sync::Arc;

use serde_json::{json, Value};
use timeline_api::aws_settings::{DeliberateFailure, EventLogging};
use timeline_api::processing::{process_upload, ProcessingError};
use timeline_api::s3_trigger::{handle_raw_s3_event_recorded, ProcessingStores};
use timeline_core::conversation_metadata::UploadFacts;
use timeline_core::labels::{FileName, PersonName};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const S3_SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");
const CONV: &str = "aaaaaaaa-0000-4000-8000-000000000001";

fn stores() -> ProcessingStores {
    ProcessingStores {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        conversation_summary_store: Arc::new(InMemoryConversationSummaryStore::new()),
        user_flag_writer: Arc::new(InMemoryMessageFlagsStore::new()),
    }
}

fn alice() -> UserId {
    UserId("alice".to_string())
}

fn export(times: &[&str]) -> String {
    let messages: Vec<Value> = times
        .iter()
        .enumerate()
        .map(|(i, at)| {
            json!({
                "uuid": format!("00000000-0000-4000-8000-{:012}", i + 1),
                "sender": "human",
                "created_at": at,
                "content": [{"type": "text", "text": format!("message {i}")}],
            })
        })
        .collect();
    json!([{ "uuid": CONV, "name": "A", "chat_messages": messages }]).to_string()
}

/// Stores `raw` as a new upload, with its facts recorded unless `record` is
/// false; returns the upload id.
async fn stored(stores: &ProcessingStores, raw: &str, record: bool) -> UploadId {
    let upload = UploadId(uuid::Uuid::new_v4());
    stores
        .object_store
        .put(&raw_object_key(&alice(), upload), raw.as_bytes().to_vec())
        .await
        .unwrap();
    if record {
        stores
            .upload_outcome_store
            .record_received(
                &alice(),
                upload,
                UploadFacts {
                    file_name: FileName::parse("a.json").unwrap(),
                    uploaded_at: chrono::Utc::now(),
                    file_written_at: None,
                    human_name: PersonName::parse("Ada").unwrap(),
                },
            )
            .await
            .unwrap();
    }
    upload
}

async fn run(stores: &ProcessingStores, upload: UploadId) -> Result<(), ProcessingError> {
    process_upload(
        stores.object_store.as_ref(),
        stores.upload_outcome_store.as_ref(),
        stores.conversation_summary_store.as_ref(),
        stores.user_flag_writer.as_ref(),
        &alice(),
        upload,
    )
    .await
}

/// Processes `upload` through the Lambda's handler; returns its run line.
async fn run_logged(stores: &ProcessingStores, upload: UploadId) -> Value {
    let mut event: Value = serde_json::from_str(S3_SAMPLE).unwrap();
    event["Records"][0]["s3"]["object"]["key"] = json!(raw_object_key(&alice(), upload));
    let runs = RefCell::new(Vec::new());
    let _ = handle_raw_s3_event_recorded(
        event,
        stores,
        EventLogging::Off,
        DeliberateFailure::Off,
        |_| {},
        &|line| {
            runs.borrow_mut()
                .push(serde_json::from_str::<Value>(line).unwrap())
        },
    )
    .await;
    runs.into_inner().remove(0)
}

#[tokio::test]
async fn a_file_no_upload_recorded_fails_with_a_reason_and_stores_nothing() {
    let stores = stores();
    let upload = stored(&stores, &export(&["2026-01-01T10:00:00Z"]), false).await;
    let err = run(&stores, upload).await.unwrap_err();
    assert!(matches!(err, ProcessingError::NoUploadRecord));
    assert!(
        err.to_string().contains("no record of this upload"),
        "{err}"
    );
    assert!(std::error::Error::source(&err).is_none());
    let outcome = stores
        .upload_outcome_store
        .get_outcome(&alice(), upload)
        .await
        .unwrap();
    assert!(
        matches!(&outcome, Some(UploadOutcome::Failed { reason }) if reason.contains("no record")),
        "{outcome:?}"
    );
    assert!(stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn on_aws_a_file_no_upload_recorded_is_marked_failed_not_retried() {
    let stores = stores();
    let upload = stored(&stores, &export(&["2026-01-01T10:00:00Z"]), false).await;
    let line = run_logged(&stores, upload).await;
    assert_eq!(line["outcome"], "unusable", "{line}");
}

#[tokio::test]
async fn processing_the_same_file_again_adds_nothing_twice() {
    // Lambda can run an attempt again after a failure part-way through.
    let stores = stores();
    run(
        &stores,
        stored(&stores, &export(&["2026-01-01T10:00:00Z"]), true).await,
    )
    .await
    .unwrap();
    let later = stored(
        &stores,
        &export(&["2026-01-01T10:00:00Z", "2026-01-02T10:00:00Z"]),
        true,
    )
    .await;
    run(&stores, later).await.unwrap();
    run(&stores, later).await.unwrap();
    let list = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].additions, vec![later]);
    assert_eq!(list[0].message_count, 2);
}

#[tokio::test]
async fn a_file_meeting_stored_conversations_logs_what_it_added() {
    let stores = stores();
    run(
        &stores,
        stored(&stores, &export(&["2026-01-01T10:00:00Z"]), true).await,
    )
    .await
    .unwrap();
    let later = stored(
        &stores,
        &export(&[
            "2026-01-01T09:00:00Z",
            "2026-01-01T10:00:00Z",
            "2026-01-03T10:00:00Z",
        ]),
        true,
    )
    .await;
    let line = run_logged(&stores, later).await;
    let facts = &line["facts"];
    assert_eq!(facts["conversations_new"], 0, "{line}");
    assert_eq!(facts["conversations_already_present"], 1);
    assert_eq!(facts["conversations_gained_messages"], 1);
    assert_eq!(facts["earliest_added_message"], "2026-01-01T09:00:00+00:00");
    assert_eq!(facts["latest_added_message"], "2026-01-03T10:00:00+00:00");
    assert!(facts.get("conversations_fewer_than_file").is_none());
}

#[tokio::test]
async fn a_file_whose_messages_fall_inside_the_stored_range_is_logged_as_missed() {
    // Plan C18: messages inside the stored range are never added, so a
    // conversation can end up with fewer messages than the file's copy.
    let stores = stores();
    run(
        &stores,
        stored(
            &stores,
            &export(&["2026-01-01T10:00:00Z", "2026-01-01T12:00:00Z"]),
            true,
        )
        .await,
    )
    .await
    .unwrap();
    let middle = stored(
        &stores,
        &export(&[
            "2026-01-01T10:00:00Z",
            "2026-01-01T11:00:00Z",
            "2026-01-01T12:00:00Z",
        ]),
        true,
    )
    .await;
    let line = run_logged(&stores, middle).await;
    assert_eq!(line["facts"]["conversations_fewer_than_file"], 1, "{line}");
    assert_eq!(line["facts"]["conversations_gained_messages"], 0);
    assert!(line["facts"].get("earliest_added_message").is_none());
}

#[tokio::test]
async fn a_stored_conversation_with_no_messages_takes_every_message_a_later_file_has() {
    let stores = stores();
    run(&stores, stored(&stores, &export(&[]), true).await)
        .await
        .unwrap();
    let later = stored(
        &stores,
        &export(&["2026-01-01T10:00:00Z", "2026-01-01T11:00:00Z"]),
        true,
    )
    .await;
    run(&stores, later).await.unwrap();
    let list = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(list[0].message_count, 2);
    let span = list[0].message_span.unwrap();
    assert_eq!(span.start().to_rfc3339(), "2026-01-01T10:00:00+00:00");
    assert_eq!(span.end().to_rfc3339(), "2026-01-01T11:00:00+00:00");
}
