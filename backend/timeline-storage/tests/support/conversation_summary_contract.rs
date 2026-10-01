//! What any correct `ConversationSummaryStore` must do, run against the
//! in-memory fake and `DynamoConversationsTable`. See the migration plan's
//! §V2b.
//!
//! The port promises no order for `list_for_user`, so these checks sort
//! before comparing. (The in-memory fake's order changes between runs; that
//! is a known defect tracked in the frontend quality-of-life plan, not
//! something this contract decides.)
//!
//! `$make` is an async fn returning `(store, keep_alive)`.

macro_rules! conversation_summary_contract {
    ($make:path) => {
        use timeline_core::model::{
            ConversationId as SummaryConversationId, ConversationName as SummaryConversationName,
        };
        use timeline_core::ports::conversations::{
            ConversationSummary as SummaryRecord, ConversationSummaryStore as SummaryStore,
        };
        use timeline_core::ports::ids::{UploadId as SummaryUploadId, UserId as SummaryUserId};

        fn summary_user(name: &str) -> SummaryUserId {
            SummaryUserId(name.to_string())
        }

        fn summary(id: u128, name: &str, message_count: usize) -> SummaryRecord {
            SummaryRecord {
                conversation_id: SummaryConversationId(uuid::Uuid::from_u128(id)),
                upload_id: SummaryUploadId(uuid::Uuid::from_u128(500)),
                name: SummaryConversationName(name.to_string()),
                message_count,
            }
        }

        fn sorted(mut list: Vec<SummaryRecord>) -> Vec<SummaryRecord> {
            list.sort_by_key(|s| s.conversation_id.0);
            list
        }

        #[tokio::test]
        async fn get_before_any_write_is_none() {
            let (store, _keep) = $make().await;
            let got = SummaryStore::get(
                &store,
                &summary_user("alice"),
                SummaryConversationId(uuid::Uuid::from_u128(1)),
            )
            .await
            .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn put_then_get_round_trips_every_field() {
            let (store, _keep) = $make().await;
            let s = summary(1, "Starting a business — “quotes” & ünïcode", 42);
            SummaryStore::put(&store, &summary_user("alice"), s.clone())
                .await
                .unwrap();
            let got = SummaryStore::get(&store, &summary_user("alice"), s.conversation_id)
                .await
                .unwrap();
            assert_eq!(got, Some(s));
        }

        #[tokio::test]
        async fn an_empty_name_and_zero_messages_round_trip() {
            let (store, _keep) = $make().await;
            let s = summary(1, "", 0);
            SummaryStore::put(&store, &summary_user("alice"), s.clone())
                .await
                .unwrap();
            let got = SummaryStore::get(&store, &summary_user("alice"), s.conversation_id)
                .await
                .unwrap();
            assert_eq!(got, Some(s));
        }

        #[tokio::test]
        async fn list_for_user_returns_every_summary_for_that_user() {
            let (store, _keep) = $make().await;
            let a = summary(1, "a", 1);
            let b = summary(2, "b", 2);
            SummaryStore::put(&store, &summary_user("alice"), a.clone())
                .await
                .unwrap();
            SummaryStore::put(&store, &summary_user("alice"), b.clone())
                .await
                .unwrap();
            let got = store.list_for_user(&summary_user("alice")).await.unwrap();
            assert_eq!(sorted(got), vec![a, b]);
        }

        #[tokio::test]
        async fn list_for_user_with_nothing_stored_is_empty() {
            let (store, _keep) = $make().await;
            let got = store.list_for_user(&summary_user("alice")).await.unwrap();
            assert!(got.is_empty(), "expected no summaries, got {got:?}");
        }

        #[tokio::test]
        async fn summaries_are_isolated_per_user() {
            let (store, _keep) = $make().await;
            SummaryStore::put(&store, &summary_user("alice"), summary(1, "a", 1))
                .await
                .unwrap();
            let listed = store.list_for_user(&summary_user("bob")).await.unwrap();
            assert!(
                listed.is_empty(),
                "bob must not see alice's summaries: {listed:?}"
            );
            let got = SummaryStore::get(
                &store,
                &summary_user("bob"),
                SummaryConversationId(uuid::Uuid::from_u128(1)),
            )
            .await
            .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn put_again_replaces_the_earlier_summary() {
            let (store, _keep) = $make().await;
            SummaryStore::put(&store, &summary_user("alice"), summary(1, "old", 1))
                .await
                .unwrap();
            let newer = summary(1, "new", 5);
            SummaryStore::put(&store, &summary_user("alice"), newer.clone())
                .await
                .unwrap();
            let got = store.list_for_user(&summary_user("alice")).await.unwrap();
            assert_eq!(got, vec![newer]);
        }
    };
}
