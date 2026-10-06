//! What any correct `UploadOutcomeStore` must do with attempt progress
//! (plan `2026-10-02-upload-processing-failures.md` §3), run against the
//! in-memory fake and `DynamoConversationsTable`.
//!
//! `$make` is an async fn returning `(store, keep_alive)`.

macro_rules! upload_progress_contract {
    ($make:path) => {
        use timeline_core::model::ConversationId as ProgressConversationId;
        use timeline_core::ports::ids::{UploadId as ProgressUploadId, UserId as ProgressUserId};
        use timeline_core::ports::uploads::{
            UploadOutcome as ProgressUploadOutcome, UploadOutcomeStore as ProgressStore,
            UploadProgress,
        };

        fn progress_user(name: &str) -> ProgressUserId {
            ProgressUserId(name.to_string())
        }

        fn progress_upload(n: u128) -> ProgressUploadId {
            ProgressUploadId(uuid::Uuid::from_u128(n))
        }

        #[tokio::test]
        async fn progress_before_any_attempt_is_none() {
            let (store, _keep) = $make().await;
            let got = store
                .get_progress(&progress_user("alice"), progress_upload(1))
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn each_attempt_is_numbered_from_one() {
            let (store, _keep) = $make().await;
            let (user, upload) = (progress_user("alice"), progress_upload(1));
            assert_eq!(store.record_attempt(&user, upload).await.unwrap(), 1);
            assert_eq!(store.record_attempt(&user, upload).await.unwrap(), 2);
            assert_eq!(
                store.get_progress(&user, upload).await.unwrap(),
                Some(UploadProgress {
                    attempts: 2,
                    last_error: None,
                    processing: None,
                })
            );
        }

        #[tokio::test]
        async fn the_last_error_is_kept_across_a_new_attempt_until_replaced() {
            let (store, _keep) = $make().await;
            let (user, upload) = (progress_user("alice"), progress_upload(1));
            store.record_attempt(&user, upload).await.unwrap();
            store
                .record_attempt_error(&user, upload, "first".to_string())
                .await
                .unwrap();
            store.record_attempt(&user, upload).await.unwrap();
            assert_eq!(
                store.get_progress(&user, upload).await.unwrap(),
                Some(UploadProgress {
                    attempts: 2,
                    last_error: Some("first".to_string()),
                    processing: None,
                })
            );
            store
                .record_attempt_error(&user, upload, "second".to_string())
                .await
                .unwrap();
            assert_eq!(
                store
                    .get_progress(&user, upload)
                    .await
                    .unwrap()
                    .unwrap()
                    .last_error
                    .as_deref(),
                Some("second")
            );
        }

        #[tokio::test]
        async fn an_error_recorded_before_any_attempt_reads_as_zero_attempts() {
            let (store, _keep) = $make().await;
            let (user, upload) = (progress_user("alice"), progress_upload(1));
            store
                .record_attempt_error(&user, upload, "early".to_string())
                .await
                .unwrap();
            assert_eq!(
                store.get_progress(&user, upload).await.unwrap(),
                Some(UploadProgress {
                    attempts: 0,
                    last_error: Some("early".to_string()),
                    processing: None,
                })
            );
        }

        #[tokio::test]
        async fn progress_is_kept_apart_per_user_and_upload_and_from_the_outcome() {
            let (store, _keep) = $make().await;
            let alice = progress_user("alice");
            store
                .record_attempt(&alice, progress_upload(1))
                .await
                .unwrap();
            assert_eq!(
                store
                    .get_progress(&alice, progress_upload(2))
                    .await
                    .unwrap(),
                None
            );
            assert_eq!(
                store
                    .get_progress(&progress_user("bob"), progress_upload(1))
                    .await
                    .unwrap(),
                None
            );
            // An attempt is not an outcome, and an outcome leaves progress alone.
            assert_eq!(
                store.get_outcome(&alice, progress_upload(1)).await.unwrap(),
                None
            );
            let ready = ProgressUploadOutcome::Ready {
                conversation_ids: vec![ProgressConversationId(uuid::Uuid::from_u128(7))],
            };
            store
                .record_outcome(&alice, progress_upload(1), ready.clone())
                .await
                .unwrap();
            assert_eq!(
                store.get_outcome(&alice, progress_upload(1)).await.unwrap(),
                Some(ready)
            );
            assert_eq!(
                store
                    .get_progress(&alice, progress_upload(1))
                    .await
                    .unwrap()
                    .unwrap()
                    .attempts,
                1
            );
        }

        /// How far the running attempt has got (plan
        /// 2026-10-06-load-only-what-the-page-shows.md §8b): written every
        /// second, each write replacing the last, beside the attempt count.
        #[tokio::test]
        async fn processing_progress_is_kept_beside_the_attempts_and_replaced_by_each_write() {
            let (store, _keep) = $make().await;
            let alice = progress_user("alice");
            let early = timeline_core::ports::uploads::ProcessingProgress {
                bytes_read: 100,
                bytes_total: 1000,
                conversations_written: 0,
                conversations_total: 0,
            };
            store
                .record_processing_progress(&alice, progress_upload(1), early)
                .await
                .unwrap();
            assert_eq!(
                store
                    .get_progress(&alice, progress_upload(1))
                    .await
                    .unwrap()
                    .unwrap()
                    .processing,
                Some(early)
            );
            store
                .record_attempt(&alice, progress_upload(1))
                .await
                .unwrap();
            let later = timeline_core::ports::uploads::ProcessingProgress {
                bytes_read: 1000,
                bytes_total: 1000,
                conversations_written: 3,
                conversations_total: 7,
            };
            store
                .record_processing_progress(&alice, progress_upload(1), later)
                .await
                .unwrap();
            let got = store
                .get_progress(&alice, progress_upload(1))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(got.attempts, 1);
            assert_eq!(got.processing, Some(later));
            assert_eq!(
                store
                    .get_progress(&progress_user("bob"), progress_upload(1))
                    .await
                    .unwrap(),
                None
            );
        }
    };
}
