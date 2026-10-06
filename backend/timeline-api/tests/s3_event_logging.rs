//! The switch that makes the processing Lambda log each S3 notification,
//! for capturing one real notification as a test sample (migration plan
//! §V2e, E9; C24). Runs `handle_raw_s3_event`, what the Lambda runs, with a
//! log function that collects lines, against the in-memory stores.
//!
//! Not covered: the binary passing `println!` and the setting it read
//! (plan C36); the capture step on AWS shows that.

#[path = "support/local_app.rs"]
mod local_app;

use std::cell::RefCell;

use serde_json::{json, Value};
use timeline_api::aws_settings::{EventLogging, InvalidEventLogging};
use timeline_api::s3_trigger::{
    handle_raw_s3_event, redact_s3_event, s3_event_log_line, ProcessingStores, RawEventError,
};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome};

const SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");
const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");
const IP: &str = "127.0.0.1"; // the sample's sourceIPAddress

fn sample() -> Value {
    serde_json::from_str(SAMPLE).unwrap()
}

/// The sample with `n` copies of its record, each with the given key.
fn with_records(key: &str, n: usize) -> Value {
    let mut event = sample();
    let mut record = event["Records"][0].clone();
    record["s3"]["object"]["key"] = json!(key);
    event["Records"] = Value::Array(vec![record; n]);
    event
}

// ---- Redaction ------------------------------------------------------------

#[test]
fn the_ip_address_is_replaced_in_every_record_and_nothing_else_changes() {
    let event = with_records("raw/a/b.json", 2);
    let redacted = redact_s3_event(event.clone());

    let mut expected = event;
    for record in expected["Records"].as_array_mut().unwrap() {
        record["requestParameters"]["sourceIPAddress"] = json!("REDACTED");
    }
    assert_eq!(redacted, expected);
    assert!(!redacted.to_string().contains(IP));
}

#[test]
fn notifications_without_the_field_or_without_records_come_back_unchanged() {
    let mut no_params = sample();
    no_params["Records"][0]
        .as_object_mut()
        .unwrap()
        .remove("requestParameters");
    assert_eq!(redact_s3_event(no_params.clone()), no_params);

    for event in [json!({"Records": []}), json!({}), json!("not an object")] {
        assert_eq!(redact_s3_event(event.clone()), event);
    }
}

#[test]
fn the_log_line_is_findable_and_carries_the_redacted_json() {
    let line = s3_event_log_line(&sample());
    let json_part = line
        .strip_prefix("s3 event (sourceIPAddress removed): ")
        .expect("line starts with the filter text");
    assert_eq!(
        serde_json::from_str::<Value>(json_part).unwrap(),
        redact_s3_event(sample())
    );
}

// ---- The setting ----------------------------------------------------------

fn setting(value: Option<&str>) -> Result<EventLogging, InvalidEventLogging> {
    EventLogging::from_lookup(|name| {
        assert_eq!(name, "TIMELINE_LOG_S3_EVENTS");
        value.map(str::to_string)
    })
}

#[test]
fn on_and_off_are_read_and_missing_means_off() {
    assert_eq!(setting(Some("on")), Ok(EventLogging::On));
    assert_eq!(setting(Some("off")), Ok(EventLogging::Off));
    assert_eq!(setting(None), Ok(EventLogging::Off));
}

#[test]
fn any_other_value_is_refused_naming_it() {
    for bad in ["ON", "yes", "true", ""] {
        let err = setting(Some(bad)).unwrap_err();
        assert_eq!(err, InvalidEventLogging(bad.to_string()));
        let text = err.to_string();
        assert!(text.contains("TIMELINE_LOG_S3_EVENTS"), "{text}");
        assert!(text.contains(&format!("{bad:?}")), "{text}");
    }
}

// ---- What the Lambda runs -------------------------------------------------

fn memory_stores() -> ProcessingStores {
    local_app::memory_stores()
}

async fn stored_upload(stores: &ProcessingStores) -> (UserId, UploadId, String) {
    let user = UserId("alice".to_string());
    let upload = UploadId(uuid::Uuid::new_v4());
    let key = raw_object_key(&user, upload);
    stores
        .object_store
        .put(&key, FIXTURE.as_bytes().to_vec())
        .await
        .unwrap();
    record_upload_facts(stores.upload_outcome_store.as_ref(), &key).await;
    (user, upload, key)
}

#[tokio::test]
async fn switched_on_it_logs_one_redacted_line_and_still_processes() {
    let stores = memory_stores();
    let (user, upload, key) = stored_upload(&stores).await;
    let lines = RefCell::new(Vec::new());

    handle_raw_s3_event(with_records(&key, 1), &stores, EventLogging::On, |l| {
        lines.borrow_mut().push(l.to_string())
    })
    .await
    .unwrap();

    let lines = lines.into_inner();
    assert_eq!(lines, vec![s3_event_log_line(&with_records(&key, 1))]);
    assert!(!lines[0].contains(IP));
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
async fn switched_off_it_logs_nothing_and_still_processes() {
    let stores = memory_stores();
    let (user, upload, key) = stored_upload(&stores).await;
    let lines = RefCell::new(Vec::new());

    handle_raw_s3_event(with_records(&key, 1), &stores, EventLogging::Off, |l| {
        lines.borrow_mut().push(l.to_string())
    })
    .await
    .unwrap();

    assert!(lines.into_inner().is_empty());
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

/// Logged before reading, so an unreadable notification still shows up in
/// the log when the switch is on.
#[tokio::test]
async fn an_unreadable_notification_is_logged_then_refused() {
    let stores = memory_stores();
    let lines = RefCell::new(Vec::new());
    let unreadable = json!({"Records": "not a list"});

    let err = handle_raw_s3_event(unreadable.clone(), &stores, EventLogging::On, |l| {
        lines.borrow_mut().push(l.to_string())
    })
    .await
    .unwrap_err();

    assert!(matches!(err, RawEventError::Unreadable(_)), "{err:?}");
    assert!(err.to_string().contains("not a readable S3 notification"));
    assert_eq!(lines.into_inner(), vec![s3_event_log_line(&unreadable)]);
}

#[tokio::test]
async fn a_failed_record_is_passed_on_as_the_triggers_error() {
    let stores = memory_stores();
    let err = handle_raw_s3_event(
        with_records("export/not-a-raw-upload.json", 1),
        &stores,
        EventLogging::Off,
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, RawEventError::Trigger(_)), "{err:?}");
    assert!(err.to_string().contains("export/not-a-raw-upload.json"));
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
