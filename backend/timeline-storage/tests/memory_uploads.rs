//! Black-box tests for `InMemoryUploadOutcomeStore`.

use timeline_core::model::ConversationId;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

fn user(id: &str) -> UserId {
    UserId(id.to_string())
}

fn upload(n: u128) -> UploadId {
    UploadId(uuid::Uuid::from_u128(n))
}

#[tokio::test]
async fn get_outcome_before_any_record_is_none_not_an_error() {
    let store = InMemoryUploadOutcomeStore::new();
    assert_eq!(
        store.get_outcome(&user("alice"), upload(1)).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn record_outcome_then_get_outcome_round_trips_ready() {
    let store = InMemoryUploadOutcomeStore::new();
    let u = user("alice");
    let id = upload(1);
    let conv_ids = vec![ConversationId(uuid::Uuid::from_u128(100))];
    store
        .record_outcome(
            &u,
            id,
            UploadOutcome::Ready {
                conversation_ids: conv_ids.clone(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        store.get_outcome(&u, id).await.unwrap(),
        Some(UploadOutcome::Ready {
            conversation_ids: conv_ids
        })
    );
}

#[tokio::test]
async fn record_outcome_then_get_outcome_round_trips_failed() {
    let store = InMemoryUploadOutcomeStore::new();
    let u = user("alice");
    let id = upload(1);
    store
        .record_outcome(
            &u,
            id,
            UploadOutcome::Failed {
                reason: "dedup blew up".to_string(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        store.get_outcome(&u, id).await.unwrap(),
        Some(UploadOutcome::Failed {
            reason: "dedup blew up".to_string()
        })
    );
}

#[tokio::test]
async fn recording_a_second_outcome_overwrites_the_first() {
    let store = InMemoryUploadOutcomeStore::new();
    let u = user("alice");
    let id = upload(1);
    store
        .record_outcome(
            &u,
            id,
            UploadOutcome::Failed {
                reason: "first attempt failed".to_string(),
            },
        )
        .await
        .unwrap();
    let conv_ids = vec![ConversationId(uuid::Uuid::from_u128(100))];
    store
        .record_outcome(
            &u,
            id,
            UploadOutcome::Ready {
                conversation_ids: conv_ids.clone(),
            },
        )
        .await
        .unwrap();

    assert_eq!(
        store.get_outcome(&u, id).await.unwrap(),
        Some(UploadOutcome::Ready {
            conversation_ids: conv_ids
        })
    );
}

#[tokio::test]
async fn outcomes_are_isolated_per_user() {
    let store = InMemoryUploadOutcomeStore::new();
    let id = upload(1);
    store
        .record_outcome(
            &user("alice"),
            id,
            UploadOutcome::Ready {
                conversation_ids: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store.get_outcome(&user("bob"), id).await.unwrap(),
        None,
        "bob must not see alice's upload outcome, even with the same id"
    );
}

#[tokio::test]
async fn reset_forgets_recorded_upload_facts() {
    use timeline_storage::memory::resettable::Resettable;
    let store = InMemoryUploadOutcomeStore::new();
    let facts = timeline_core::conversation_metadata::UploadFacts {
        file_name: timeline_core::labels::FileName::parse("a.json").unwrap(),
        uploaded_at: chrono::Utc::now(),
        file_written_at: None,
        human_name: timeline_core::labels::PersonName::parse("Ada").unwrap(),
    };
    store
        .record_received(&user("alice"), upload(1), facts)
        .await
        .unwrap();
    store.reset();
    assert_eq!(
        store.get_received(&user("alice"), upload(1)).await.unwrap(),
        None
    );
}
