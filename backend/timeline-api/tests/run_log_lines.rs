//! The log lines written by the two functions that aren't the API: one
//! `processing_run` line per S3 record the processing function handles, and
//! one `upload_marked_failed` line per upload the failure recorder marks
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3). Also the
//! activity function's own settings, `LoginSettings`.
//!
//! Proves, through the public handlers the Lambda binaries run: the run line
//! is written whether or not S3 notifications are being logged, and on a
//! separate channel from them (`handle_raw_s3_event_recorded`'s `run_log`);
//! what processing read and stored reaches the line from inside
//! `processing::process_upload` (its `request_record::note` calls); and each
//! outcome -- ready, unusable file, error -- is named.

use std::cell::RefCell;
use std::sync::Arc;

use serde_json::{json, Value};
use timeline_api::aws_settings::{DeliberateFailure, EventLogging, LoginSettings, MissingSettings};
use timeline_api::failed_upload::handle_failed_invocation_logged;
use timeline_api::s3_trigger::{handle_raw_s3_event_recorded, ProcessingStores};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::raw_object_key;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const S3_SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");
const FAILURE_SAMPLE: &str = include_str!("fixtures/aws-samples/example-destination-failure.json");
const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");

fn stores() -> ProcessingStores {
    ProcessingStores {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        conversation_summary_store: Arc::new(InMemoryConversationSummaryStore::new()),
        user_flag_writer: Arc::new(InMemoryMessageFlagsStore::new()),
    }
}

/// An S3 notification naming `keys`; `None` is a record with no key.
fn notification(keys: &[Option<&str>]) -> Value {
    let mut event: Value = serde_json::from_str(S3_SAMPLE).unwrap();
    let record = event["Records"][0].clone();
    event["Records"] = keys
        .iter()
        .map(|key| {
            let mut r = record.clone();
            match key {
                Some(key) => r["s3"]["object"]["key"] = json!(key),
                None => {
                    r["s3"]["object"].as_object_mut().unwrap().remove("key");
                }
            }
            r
        })
        .collect();
    event
}

/// Runs the processing handler as the Lambda does; returns the notification
/// log lines and the run lines separately.
async fn process(
    stores: &ProcessingStores,
    event: Value,
    logging: EventLogging,
) -> (Vec<String>, Vec<Value>) {
    let logged = RefCell::new(Vec::new());
    let runs = RefCell::new(Vec::new());
    let _result = handle_raw_s3_event_recorded(
        event,
        stores,
        logging,
        DeliberateFailure::Off,
        |line| logged.borrow_mut().push(line.to_string()),
        &|line| runs.borrow_mut().push(serde_json::from_str(line).unwrap()),
    )
    .await;
    (logged.into_inner(), runs.into_inner())
}

#[tokio::test]
async fn a_ready_upload_logs_what_was_read_and_stored_on_its_own_channel() {
    let stores = stores();
    let user = UserId("alice".to_string());
    let upload = UploadId(uuid::Uuid::new_v4());
    let key = raw_object_key(&user, upload);
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();

    let (logged, runs) = process(&stores, notification(&[Some(&key)]), EventLogging::Off).await;

    assert!(logged.is_empty(), "notification logging is off");
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run["kind"], "processing_run");
    assert_eq!(run["key"], key);
    assert_eq!(run["user"], "alice");
    assert_eq!(run["upload_id"], upload.0.to_string());
    assert_eq!(run["outcome"], "ready");
    assert_eq!(run["error"], Value::Null);
    assert_eq!(
        run["facts"],
        json!({ "attempt": 1, "bytes": FIXTURE.len(), "reviews": 0, "conversations": 6 })
    );
    assert!(run["ms"].is_u64());
    assert_eq!(run["aws_calls"], json!({}));
}

#[tokio::test]
async fn with_notification_logging_on_both_channels_get_their_own_line() {
    let stores = stores();
    let key = raw_object_key(&UserId("alice".to_string()), UploadId(uuid::Uuid::new_v4()));
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();

    let (logged, runs) = process(&stores, notification(&[Some(&key)]), EventLogging::On).await;

    assert_eq!(logged.len(), 1);
    assert!(logged[0].starts_with("s3 event"), "{}", logged[0]);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["outcome"], "ready");
}

#[tokio::test]
async fn an_unusable_file_and_unusable_keys_are_named_in_their_lines() {
    let stores = stores();
    let key = raw_object_key(&UserId("alice".to_string()), UploadId(uuid::Uuid::new_v4()));
    stores
        .object_store
        .put(&key, b"not an export".to_vec())
        .await
        .unwrap();

    let event = notification(&[Some(&key), Some("export/elsewhere.json"), None]);
    let (_, runs) = process(&stores, event, EventLogging::Off).await;

    assert_eq!(runs.len(), 3);
    assert_eq!(runs[0]["outcome"], "unusable");
    assert!(runs[0]["error"].as_str().unwrap().len() > 0);
    assert_eq!(runs[0]["facts"]["attempt"], 1);
    assert_eq!(runs[1]["outcome"], "error");
    assert!(runs[1]["error"]
        .as_str()
        .unwrap()
        .contains("is not a raw upload"));
    assert_eq!(runs[1]["user"], Value::Null);
    assert_eq!(runs[2]["outcome"], "error");
    assert_eq!(runs[2]["key"], Value::Null);
    assert!(runs[2]["error"].as_str().unwrap().contains("no object key"));
}

fn failure_record(keys: &[&str]) -> Value {
    let mut record: Value = serde_json::from_str(FAILURE_SAMPLE).unwrap();
    let s3_record = record["requestPayload"]["Records"][0].clone();
    record["requestPayload"]["Records"] = keys
        .iter()
        .map(|key| {
            let mut r = s3_record.clone();
            r["s3"]["object"]["key"] = json!(key);
            r
        })
        .collect();
    record
}

#[tokio::test]
async fn each_upload_marked_failed_gets_a_line_naming_its_outcome() {
    let store = InMemoryUploadOutcomeStore::new();
    let upload = UploadId(uuid::Uuid::from_u128(7));
    let good = raw_object_key(&UserId("alice".to_string()), upload);
    let lines = RefCell::new(Vec::new());

    let result = handle_failed_invocation_logged(
        failure_record(&[&good, "not/a/raw%ZZkey"]),
        &store,
        &|line| {
            lines
                .borrow_mut()
                .push(serde_json::from_str::<Value>(line).unwrap())
        },
    )
    .await;

    assert!(result.is_err(), "the unusable key is an error");
    let lines = lines.into_inner();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["kind"], "upload_marked_failed");
    assert_eq!(lines[0]["key"], good);
    assert_eq!(lines[0]["user"], "alice");
    assert_eq!(lines[0]["upload_id"], upload.0.to_string());
    assert_eq!(lines[0]["outcome"], "recorded");
    assert_eq!(lines[0]["error"], Value::Null);
    assert!(lines[0]["reason"].as_str().unwrap().contains("after"));
    assert_eq!(lines[1]["outcome"], "error");
    assert_eq!(lines[1]["key"], "not/a/raw%ZZkey", "logged as it arrived");
    assert_eq!(lines[1]["user"], Value::Null);
    assert!(lines[1]["error"]
        .as_str()
        .unwrap()
        .contains("is not a raw upload"));
}

#[test]
fn login_settings_read_the_three_cognito_settings_and_name_any_missing() {
    let settings = LoginSettings::from_lookup(|name| match name {
        "TIMELINE_COGNITO_USER_POOL_ID" => Some("us-east-1_AbC".to_string()),
        "TIMELINE_COGNITO_CLIENT_ID" => Some("client123".to_string()),
        "AWS_REGION" => Some("us-east-1".to_string()),
        _ => None,
    })
    .unwrap();
    assert_eq!(
        settings.issuer(),
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbC"
    );
    assert_eq!(
        settings.jwks_url(),
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbC/.well-known/jwks.json"
    );
    assert_eq!(settings.client_id.as_str(), "client123");

    let missing = LoginSettings::from_lookup(|_| None).unwrap_err();
    assert_eq!(
        missing,
        MissingSettings(vec![
            "TIMELINE_COGNITO_USER_POOL_ID",
            "TIMELINE_COGNITO_CLIENT_ID",
            "AWS_REGION"
        ])
    );
}
