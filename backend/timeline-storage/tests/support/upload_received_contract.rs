//! What any correct `UploadOutcomeStore` must do with what `POST /uploads`
//! learned about a file (plan `2026-10-05-screen-flow.md` §8b), run against
//! the in-memory fake and `DynamoConversationsTable`.
//!
//! `$make` is an async fn returning `(store, keep_alive)`.

macro_rules! upload_received_contract {
    ($make:path) => {
        use timeline_core::conversation_metadata::UploadFacts as ReceivedFacts;
        use timeline_core::labels::{
            FileName as ReceivedFileName, PersonName as ReceivedPersonName,
        };
        use timeline_core::ports::ids::{UploadId as ReceivedUploadId, UserId as ReceivedUserId};
        use timeline_core::ports::uploads::UploadOutcomeStore as ReceivedStore;

        fn received_user(name: &str) -> ReceivedUserId {
            ReceivedUserId(name.to_string())
        }

        fn received_upload(n: u128) -> ReceivedUploadId {
            ReceivedUploadId(uuid::Uuid::from_u128(n))
        }

        fn received_facts(file: &str, written: Option<i64>) -> ReceivedFacts {
            ReceivedFacts {
                file_name: ReceivedFileName::parse(file).unwrap(),
                uploaded_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
                file_written_at: written.map(|s| chrono::DateTime::from_timestamp(s, 0).unwrap()),
                human_name: ReceivedPersonName::parse("ada@example.com").unwrap(),
            }
        }

        #[tokio::test]
        async fn facts_never_recorded_are_none() {
            let (store, _keep) = $make().await;
            let got = store
                .get_received(&received_user("alice"), received_upload(1))
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn recorded_facts_read_back_exactly() {
            let (store, _keep) = $make().await;
            let facts = received_facts("export 1.json", Some(1_699_000_000));
            store
                .record_received(&received_user("alice"), received_upload(1), facts.clone())
                .await
                .unwrap();
            let got = store
                .get_received(&received_user("alice"), received_upload(1))
                .await
                .unwrap();
            assert_eq!(got, Some(facts));
        }

        #[tokio::test]
        async fn facts_are_kept_per_user_and_per_upload() {
            let (store, _keep) = $make().await;
            store
                .record_received(
                    &received_user("alice"),
                    received_upload(1),
                    received_facts("a.json", None),
                )
                .await
                .unwrap();
            store
                .record_received(
                    &received_user("alice"),
                    received_upload(2),
                    received_facts("b.json", None),
                )
                .await
                .unwrap();
            let bob = store
                .get_received(&received_user("bob"), received_upload(1))
                .await
                .unwrap();
            assert_eq!(bob, None);
            let second = store
                .get_received(&received_user("alice"), received_upload(2))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(second.file_name.as_str(), "b.json");
        }

        #[tokio::test]
        async fn facts_do_not_disturb_the_outcome_or_progress() {
            let (store, _keep) = $make().await;
            store
                .record_received(
                    &received_user("alice"),
                    received_upload(1),
                    received_facts("a.json", None),
                )
                .await
                .unwrap();
            assert_eq!(
                store
                    .get_outcome(&received_user("alice"), received_upload(1))
                    .await
                    .unwrap(),
                None
            );
            assert_eq!(
                store
                    .get_progress(&received_user("alice"), received_upload(1))
                    .await
                    .unwrap(),
                None
            );
        }
    };
}
