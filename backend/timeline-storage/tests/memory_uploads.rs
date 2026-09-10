//! Black-box tests for `InMemoryUploadStore`.

use timeline_core::model::ConversationId;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadStatus, UploadStore};
use timeline_storage::memory::uploads::InMemoryUploadStore;

fn user(id: &str) -> UserId {
    UserId(id.to_string())
}

fn upload(n: u128) -> UploadId {
    UploadId(uuid::Uuid::from_u128(n))
}

#[tokio::test]
async fn create_pending_then_get_round_trips() {
    let store = InMemoryUploadStore::new();
    let u = user("alice");
    let id = upload(1);
    store
        .create_pending(&u, id, "raw/alice/1.json")
        .await
        .unwrap();

    let record = store.get(&u, id).await.unwrap().expect("record must exist");
    assert_eq!(record.status, UploadStatus::Pending);
    assert_eq!(record.raw_object_key, "raw/alice/1.json");
}

#[tokio::test]
async fn get_before_creation_is_none_not_an_error() {
    let store = InMemoryUploadStore::new();
    assert_eq!(store.get(&user("alice"), upload(1)).await.unwrap(), None);
}

#[tokio::test]
async fn lifecycle_pending_to_processing_to_ready() {
    let store = InMemoryUploadStore::new();
    let u = user("alice");
    let id = upload(1);
    store
        .create_pending(&u, id, "raw/alice/1.json")
        .await
        .unwrap();

    store.mark_processing(&u, id).await.unwrap();
    assert_eq!(
        store.get(&u, id).await.unwrap().unwrap().status,
        UploadStatus::Processing
    );

    let conv_ids = vec![ConversationId(uuid::Uuid::from_u128(100))];
    store.mark_ready(&u, id, conv_ids.clone()).await.unwrap();
    assert_eq!(
        store.get(&u, id).await.unwrap().unwrap().status,
        UploadStatus::Ready {
            conversation_ids: conv_ids
        }
    );
}

#[tokio::test]
async fn lifecycle_can_end_in_failure() {
    let store = InMemoryUploadStore::new();
    let u = user("alice");
    let id = upload(1);
    store
        .create_pending(&u, id, "raw/alice/1.json")
        .await
        .unwrap();

    store
        .mark_failed(&u, id, "dedup blew up".to_string())
        .await
        .unwrap();
    assert_eq!(
        store.get(&u, id).await.unwrap().unwrap().status,
        UploadStatus::Failed {
            reason: "dedup blew up".to_string()
        }
    );
}

#[tokio::test]
async fn transitioning_an_upload_that_was_never_created_is_not_found() {
    let store = InMemoryUploadStore::new();
    let err = store
        .mark_processing(&user("alice"), upload(1))
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::NotFound));
}

#[tokio::test]
async fn uploads_are_isolated_per_user() {
    let store = InMemoryUploadStore::new();
    let id = upload(1);
    store
        .create_pending(&user("alice"), id, "raw/alice/1.json")
        .await
        .unwrap();
    assert_eq!(
        store.get(&user("bob"), id).await.unwrap(),
        None,
        "bob must not see alice's upload, even with the same id"
    );
}
