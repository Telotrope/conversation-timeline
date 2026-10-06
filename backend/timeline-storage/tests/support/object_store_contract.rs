//! What any correct `ObjectStore` must do, written once and run against
//! every implementation (the in-memory fake and `S3ObjectStore`), so a fake
//! that drifts from the real service fails a test. See the migration plan's
//! §V2b "Tests".
//!
//! `$make` is an async fn returning `(store, keep_alive)`; `keep_alive`
//! holds whatever must outlive the test (a local server, a temp folder).
//!
//! Presigned URLs aren't covered here: the in-memory fake returns relative
//! paths that only `timeline-api`'s `_dev` routes can serve, so the HTTP
//! round-trip checks live in the S3-specific test file.

macro_rules! object_store_contract {
    ($make:path) => {
        use timeline_core::ports::errors::ObjectStoreError as ContractObjectStoreError;
        use timeline_core::ports::object_store::ObjectStore as ContractObjectStore;

        #[tokio::test]
        async fn put_then_get_round_trips() {
            let (store, _keep) = $make().await;
            store.put("k", b"hello".to_vec()).await.unwrap();
            assert_eq!(store.get("k").await.unwrap(), b"hello");
        }

        #[tokio::test]
        async fn get_missing_key_is_not_found() {
            let (store, _keep) = $make().await;
            let result = store.get("missing").await;
            assert!(
                matches!(result, Err(ContractObjectStoreError::NotFound)),
                "expected NotFound, got {result:?}"
            );
        }

        #[tokio::test]
        async fn put_overwrites_an_existing_object_at_the_same_key() {
            let (store, _keep) = $make().await;
            store.put("k", b"first".to_vec()).await.unwrap();
            store.put("k", b"second".to_vec()).await.unwrap();
            assert_eq!(store.get("k").await.unwrap(), b"second");
        }

        #[tokio::test]
        async fn keys_in_the_real_raw_upload_format_round_trip() {
            let (store, _keep) = $make().await;
            let key = timeline_core::ports::uploads::raw_object_key(
                &timeline_core::ports::ids::UserId("alice@example.com".to_string()),
                timeline_core::ports::ids::UploadId(uuid::Uuid::from_u128(7)),
            );
            store.put(&key, b"{}".to_vec()).await.unwrap();
            assert_eq!(store.get(&key).await.unwrap(), b"{}");
        }

        #[tokio::test]
        async fn a_multi_megabyte_object_round_trips_byte_for_byte() {
            let (store, _keep) = $make().await;
            let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
            store.put("big", data.clone()).await.unwrap();
            assert_eq!(store.get("big").await.unwrap(), data);
        }

        #[tokio::test]
        async fn presigned_put_and_get_urls_differ() {
            let (store, _keep) = $make().await;
            let ttl = std::time::Duration::from_secs(60);
            let put_url = store.presign_put("k", ttl).await.unwrap();
            let get_url = store.presign_get("k", ttl).await.unwrap();
            assert_ne!(put_url, get_url);
        }

        #[tokio::test]
        async fn a_deleted_object_is_gone_and_deleting_a_missing_one_is_no_error() {
            let (store, _keep) = $make().await;
            store.put("gone", b"x".to_vec()).await.unwrap();
            store.put("kept", b"y".to_vec()).await.unwrap();
            store.delete("gone").await.unwrap();
            store.delete("never-written").await.unwrap();
            assert!(matches!(
                store.get("gone").await,
                Err(timeline_core::ports::errors::ObjectStoreError::NotFound)
            ));
            assert_eq!(store.get("kept").await.unwrap(), b"y");
        }
    };
}
