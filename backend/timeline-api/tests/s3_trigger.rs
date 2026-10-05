//! The upload-processing Lambda's handler, fed S3 events and run against
//! the local stand-ins (S3 by `s3s-fs`, DynamoDB by DynamoDB Local) through
//! the same stores `build_processing_stores` gives the deployed function.
//! See the migration plan's §V2e, E2.
//!
//! The events start from the sample S3 event shipped with
//! `aws_lambda_events`, with only the bucket and key changed; see
//! `tests/fixtures/aws-samples/README.md` and the plan's C24.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them.

#[path = "support/aws_world.rs"]
#[allow(dead_code)]
mod aws_world;
#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "../../timeline-storage/tests/support/s3_local.rs"]
#[allow(dead_code)]
mod s3_local;

use aws_lambda_events::event::s3::S3Event;
use serde_json::{json, Value};
use timeline_api::aws_state::build_processing_stores;
use timeline_api::s3_trigger::{handle_s3_event, ProcessingStores, RecordError};
use timeline_core::ports::conversations::ConversationSummary;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome};

use aws_world::{World, FIXTURE};

const SAMPLE: &str = include_str!("fixtures/aws-samples/example-s3-event.json");

/// The sample event with one record per key; `None` removes the key.
fn event(keys: &[Option<&str>]) -> S3Event {
    let sample: Value = serde_json::from_str(SAMPLE).unwrap();
    let template = sample["Records"][0].clone();
    let records: Vec<Value> = keys
        .iter()
        .map(|key| {
            let mut record = template.clone();
            record["s3"]["bucket"]["name"] = json!(s3_local::BUCKET);
            record["s3"]["object"]["key"] = match key {
                Some(k) => json!(k),
                None => Value::Null,
            };
            record
        })
        .collect();
    serde_json::from_value(json!({ "Records": records })).unwrap()
}

fn stores(world: &World) -> ProcessingStores {
    build_processing_stores(&world.storage_settings(), world.clients())
}

/// Stores `content` as a raw upload for `user`; returns its id and key.
async fn put_upload(stores: &ProcessingStores, user: &UserId, content: &str) -> (UploadId, String) {
    let upload_id = UploadId(uuid::Uuid::new_v4());
    let key = raw_object_key(user, upload_id);
    stores
        .object_store
        .put(&key, content.as_bytes().to_vec())
        .await
        .unwrap();
    record_upload_facts(stores.upload_outcome_store.as_ref(), &key).await;
    (upload_id, key)
}

async fn summaries(stores: &ProcessingStores, user: &UserId) -> Vec<ConversationSummary> {
    let mut list = stores
        .conversation_summary_store
        .list_for_user(user)
        .await
        .unwrap();
    list.sort_by_key(|s| s.conversation_id.0);
    list
}

fn fixture_conversation_count() -> usize {
    serde_json::from_str::<Value>(FIXTURE)
        .unwrap()
        .as_array()
        .unwrap()
        .len()
}

#[tokio::test]
async fn a_valid_upload_stores_its_summaries_and_a_ready_outcome() {
    let world = World::new().await;
    let stores = stores(&world);
    let alice = UserId("alice".to_string());
    let (upload_id, key) = put_upload(&stores, &alice, FIXTURE).await;

    handle_s3_event(event(&[Some(&key)]), &stores)
        .await
        .unwrap();

    assert_eq!(
        summaries(&stores, &alice).await.len(),
        fixture_conversation_count()
    );
    let outcome = stores
        .upload_outcome_store
        .get_outcome(&alice, upload_id)
        .await
        .unwrap();
    assert!(
        matches!(outcome, Some(UploadOutcome::Ready { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_file_that_is_not_an_export_is_recorded_as_failed_and_not_retried() {
    let world = World::new().await;
    let stores = stores(&world);
    let alice = UserId("alice".to_string());
    let (upload_id, key) = put_upload(&stores, &alice, "not an export").await;

    // Success: retrying the same bytes can't help.
    handle_s3_event(event(&[Some(&key)]), &stores)
        .await
        .unwrap();

    let outcome = stores
        .upload_outcome_store
        .get_outcome(&alice, upload_id)
        .await
        .unwrap();
    assert!(
        matches!(outcome, Some(UploadOutcome::Failed { .. })),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn keys_that_are_not_raw_uploads_are_errors_naming_the_key() {
    let world = World::new().await;
    let stores = stores(&world);
    let export_key = "export/alice/6f1c1b6e-2f4e-4a8e-9a57-0b6f1d6a9c11.json";
    let bad_upload = "raw/alice/not-a-uuid.json";
    let undecodable = "raw/alice/%FF.json";

    let err = handle_s3_event(
        event(&[Some(export_key), Some(bad_upload), Some(undecodable), None]),
        &stores,
    )
    .await
    .unwrap_err();

    assert_eq!(err.0.len(), 4);
    assert!(matches!(&err.0[0], RecordError::UnusableKey { key, .. } if key == export_key));
    assert!(matches!(&err.0[1], RecordError::UnusableKey { key, .. } if key == bad_upload));
    assert!(matches!(&err.0[2], RecordError::UnusableKey { key, .. } if key == undecodable));
    assert!(matches!(err.0[3], RecordError::MissingKey));
    let text = err.to_string();
    for expected in [
        "4 record(s) failed",
        export_key,
        bad_upload,
        "not valid UTF-8",
        "no object key",
    ] {
        assert!(text.contains(expected), "{text} should mention {expected}");
    }
}

/// S3 encodes keys in events: `%XX` for special characters and `+` for a
/// space. The handler must decode before tracing the key to its upload.
#[tokio::test]
async fn encoded_keys_are_decoded_before_parsing() {
    let world = World::new().await;
    let stores = stores(&world);
    let hyphen = UserId("alice-smith".to_string());
    let space = UserId("bob jones".to_string());
    let (hyphen_upload, hyphen_key) = put_upload(&stores, &hyphen, FIXTURE).await;
    let (space_upload, space_key) = put_upload(&stores, &space, FIXTURE).await;

    let encoded_hyphen = hyphen_key.replace("alice-smith", "alice%2Dsmith");
    let encoded_space = space_key.replace("bob jones", "bob+jones");
    handle_s3_event(
        event(&[Some(&encoded_hyphen), Some(&encoded_space)]),
        &stores,
    )
    .await
    .unwrap();

    for (user, upload) in [(&hyphen, hyphen_upload), (&space, space_upload)] {
        let outcome = stores
            .upload_outcome_store
            .get_outcome(user, upload)
            .await
            .unwrap();
        assert!(
            matches!(outcome, Some(UploadOutcome::Ready { .. })),
            "{user}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn one_missing_file_does_not_stop_the_other_records() {
    let world = World::new().await;
    let stores = stores(&world);
    let alice = UserId("alice".to_string());
    let (upload_id, present) = put_upload(&stores, &alice, FIXTURE).await;
    let missing = raw_object_key(&alice, UploadId(uuid::Uuid::new_v4()));

    let err = handle_s3_event(event(&[Some(&missing), Some(&present)]), &stores)
        .await
        .unwrap_err();

    assert_eq!(err.0.len(), 1);
    assert!(matches!(&err.0[0], RecordError::Processing { key, .. } if *key == missing));
    assert!(err.to_string().contains(&missing));
    let outcome = stores
        .upload_outcome_store
        .get_outcome(&alice, upload_id)
        .await
        .unwrap();
    assert!(
        matches!(outcome, Some(UploadOutcome::Ready { .. })),
        "{outcome:?}"
    );
}

/// Lambda retries a failed event, so the same upload can be processed more
/// than once (plan C29).
#[tokio::test]
async fn processing_an_event_twice_stores_the_same_data_as_once() {
    let world = World::new().await;
    let stores = stores(&world);
    let alice = UserId("alice".to_string());
    let (upload_id, key) = put_upload(&stores, &alice, FIXTURE).await;

    handle_s3_event(event(&[Some(&key)]), &stores)
        .await
        .unwrap();
    let once = summaries(&stores, &alice).await;
    let outcome_once = stores
        .upload_outcome_store
        .get_outcome(&alice, upload_id)
        .await
        .unwrap();

    handle_s3_event(event(&[Some(&key)]), &stores)
        .await
        .unwrap();
    assert_eq!(summaries(&stores, &alice).await, once);
    assert_eq!(
        stores
            .upload_outcome_store
            .get_outcome(&alice, upload_id)
            .await
            .unwrap(),
        outcome_once
    );
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
