//! Black-box tests for `InMemoryObjectStore`, calling only the crate's
//! public API.

use std::time::Duration;

use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;

#[tokio::test]
async fn put_then_get_round_trips() {
    let store = InMemoryObjectStore::new();
    store.put("k", b"hello".to_vec()).await.unwrap();
    assert_eq!(store.get("k").await.unwrap(), b"hello");
}

#[tokio::test]
async fn get_missing_key_is_not_found() {
    let store = InMemoryObjectStore::new();
    assert!(matches!(
        store.get("missing").await,
        Err(ObjectStoreError::NotFound)
    ));
}

#[tokio::test]
async fn presigned_urls_are_distinct_for_put_and_get() {
    let store = InMemoryObjectStore::new();
    let put_url = store
        .presign_put("k", Duration::from_secs(60))
        .await
        .unwrap();
    let get_url = store
        .presign_get("k", Duration::from_secs(60))
        .await
        .unwrap();
    assert_ne!(put_url, get_url);
}

#[tokio::test]
async fn put_overwrites_an_existing_object_at_the_same_key() {
    let store = InMemoryObjectStore::new();
    store.put("k", b"first".to_vec()).await.unwrap();
    store.put("k", b"second".to_vec()).await.unwrap();
    assert_eq!(store.get("k").await.unwrap(), b"second");
}
