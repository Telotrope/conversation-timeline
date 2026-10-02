//! `handle_failed_invocation`: after the processing function's last retry
//! fails, AWS hands its invocation record to `record_failed_upload`, which
//! records `Failed` so the page stops waiting (plan
//! `2026-10-02-upload-processing-failures.md` §2).
//!
//! The sample record is written from AWS's documentation, not captured from
//! this project (fixtures README; plan C5).

use async_trait::async_trait;
use serde_json::{json, Value};
use timeline_api::failed_upload::{handle_failed_invocation, FailedUploadError};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{
    raw_object_key, UploadOutcome, UploadOutcomeStore, UploadProgress,
};
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const SAMPLE: &str = include_str!("fixtures/aws-samples/example-destination-failure.json");
const SAMPLE_MESSAGE: &str =
    "RequestId: e4b46cbf-b738-xmpl-8880-a18cdf61200e Process exited before completing request";

fn user() -> UserId {
    UserId("alice".to_string())
}

fn upload(n: u128) -> UploadId {
    UploadId(uuid::Uuid::from_u128(n))
}

/// The sample with one S3 record per key.
fn record_for(keys: &[&str]) -> Value {
    let mut record: Value = serde_json::from_str(SAMPLE).unwrap();
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

async fn failure_reason(store: &InMemoryUploadOutcomeStore, n: u128) -> String {
    match store.get_outcome(&user(), upload(n)).await.unwrap() {
        Some(UploadOutcome::Failed { reason }) => reason,
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn each_upload_named_is_recorded_as_failed_with_the_attempts_and_the_error() {
    let store = InMemoryUploadOutcomeStore::new();
    let (a, b) = (raw_object_key(&user(), upload(1)), raw_object_key(&user(), upload(2)));

    handle_failed_invocation(record_for(&[&a, &b]), &store).await.unwrap();

    let expected = format!("the server couldn't process the file after 3 attempts: {SAMPLE_MESSAGE}");
    assert_eq!(failure_reason(&store, 1).await, expected);
    assert_eq!(failure_reason(&store, 2).await, expected);
}

#[tokio::test]
async fn without_a_message_the_reason_falls_back_to_the_type_then_the_condition() {
    let key = raw_object_key(&user(), upload(1));
    let cases = [
        (json!({"errorType": "Runtime.ExitError"}), "Runtime.ExitError"),
        (json!({}), "RetriesExhausted"),
        (Value::Null, "RetriesExhausted"),
    ];
    for (response, expected) in cases {
        let store = InMemoryUploadOutcomeStore::new();
        let mut record = record_for(&[&key]);
        record["responsePayload"] = response;
        handle_failed_invocation(record, &store).await.unwrap();
        assert!(failure_reason(&store, 1).await.ends_with(&format!(": {expected}")));
    }

    let store = InMemoryUploadOutcomeStore::new();
    let mut record = record_for(&[&key]);
    record["responsePayload"] = Value::Null;
    record["requestContext"] = json!({});
    handle_failed_invocation(record, &store).await.unwrap();
    assert_eq!(
        failure_reason(&store, 1).await,
        "the server couldn't process the file after 3 attempts: no error message was given"
    );
}

#[tokio::test]
async fn the_attempt_count_comes_from_aws_when_it_says() {
    let store = InMemoryUploadOutcomeStore::new();
    let mut record = record_for(&[&raw_object_key(&user(), upload(1))]);
    record["requestContext"]["approximateInvokeCount"] = json!(2);
    handle_failed_invocation(record, &store).await.unwrap();
    assert!(failure_reason(&store, 1).await.contains("after 2 attempts"));
}

#[tokio::test]
async fn an_unusable_key_is_an_error_naming_it_and_the_other_keys_are_still_recorded() {
    let store = InMemoryUploadOutcomeStore::new();
    let good = raw_object_key(&user(), upload(1));

    let err = handle_failed_invocation(record_for(&["export/alice/x.json", "raw/%FF", &good]), &store)
        .await
        .unwrap_err();

    assert!(matches!(&err, FailedUploadError::UnusableKey { key, .. } if key == "export/alice/x.json"));
    assert_eq!(
        err.to_string(),
        "object key \"export/alice/x.json\" is not a raw upload: expected raw/<user>/<upload uuid>.json"
    );
    assert!(failure_reason(&store, 1).await.starts_with("the server couldn't process"));

    // A key that isn't valid once decoded names the raw key.
    let err = handle_failed_invocation(record_for(&["raw/%FF"]), &store).await.unwrap_err();
    assert!(matches!(&err, FailedUploadError::UnusableKey { key, .. } if key == "raw/%FF"), "{err}");
}

#[tokio::test]
async fn input_that_is_not_an_invocation_record_of_an_s3_notification_is_an_error() {
    let store = InMemoryUploadOutcomeStore::new();

    let err = handle_failed_invocation(json!({"Records": []}), &store).await.unwrap_err();
    assert!(matches!(err, FailedUploadError::NotAnInvocationRecord));
    assert_eq!(err.to_string(), "not a Lambda invocation record: there is no requestPayload");

    let err = handle_failed_invocation(json!({"requestPayload": {"Records": 5}}), &store)
        .await
        .unwrap_err();
    assert!(matches!(err, FailedUploadError::UnreadableNotification(_)));
    assert!(err.to_string().starts_with("requestPayload is not a readable S3 notification: "), "{err}");
}

struct OutcomesFail;

#[async_trait]
impl UploadOutcomeStore for OutcomesFail {
    async fn record_outcome(&self, _: &UserId, _: UploadId, _: UploadOutcome) -> Result<(), StoreError> {
        Err(StoreError::NotFound)
    }
    async fn get_outcome(&self, _: &UserId, _: UploadId) -> Result<Option<UploadOutcome>, StoreError> {
        unreachable!("not read by handle_failed_invocation")
    }
    async fn record_attempt(&self, _: &UserId, _: UploadId) -> Result<usize, StoreError> {
        unreachable!("not used by handle_failed_invocation")
    }
    async fn record_attempt_error(&self, _: &UserId, _: UploadId, _: String) -> Result<(), StoreError> {
        unreachable!("not used by handle_failed_invocation")
    }
    async fn get_progress(&self, _: &UserId, _: UploadId) -> Result<Option<UploadProgress>, StoreError> {
        unreachable!("not used by handle_failed_invocation")
    }
}

#[tokio::test]
async fn a_failed_write_is_an_error_naming_the_key() {
    let key = raw_object_key(&user(), upload(1));
    let err = handle_failed_invocation(record_for(&[&key]), &OutcomesFail).await.unwrap_err();
    assert!(matches!(&err, FailedUploadError::Store { .. }));
    assert_eq!(err.to_string(), format!("recording {key:?} as failed: item not found"));
}
