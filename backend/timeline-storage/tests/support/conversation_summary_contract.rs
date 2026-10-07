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
                name: SummaryConversationName(name.to_string()),
                version: 0,
                source: timeline_core::conversation_metadata::SourceFile {
                    upload_id: SummaryUploadId(uuid::Uuid::from_u128(500)),
                    file_name: timeline_core::labels::FileName::parse("conversations.json")
                        .unwrap(),
                    uploaded_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
                    file_written_at: None,
                },
                additions: Vec::new(),
                message_count: message_count,
                untimed: 0,
                out_of_order: 0,
                message_span: None,
                participants: timeline_core::conversation_metadata::Participants::new(vec![
                    timeline_core::conversation_metadata::Participant::Claude,
                ])
                .unwrap(),
                medium: timeline_core::conversation_metadata::ConversationMedium::Typed,
                details_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
                span: timeline_core::conversation_metadata::ConversationSpan::new(
                    chrono::DateTime::from_timestamp(1_700_000_000, 0)
                        .unwrap()
                        .fixed_offset(),
                    chrono::DateTime::from_timestamp(1_700_003_600, 0)
                        .unwrap()
                        .fixed_offset(),
                )
                .unwrap(),
                span_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
                branch_of: None,
                branches: Vec::new(),
            }
        }

        /// `s` as `put` stores it: the version read, raised by one.
        fn as_stored(mut s: SummaryRecord) -> SummaryRecord {
            s.version += 1;
            s
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
            let mut s = summary(1, "Starting a business — “quotes” & ünïcode", 42);
            s.untimed = 3;
            s.branch_of = Some(SummaryConversationId(uuid::Uuid::from_u128(77)));
            s.branches = vec![SummaryConversationId(uuid::Uuid::from_u128(78))];
            let returned = SummaryStore::put(&store, &summary_user("alice"), s.clone())
                .await
                .unwrap();
            assert_eq!(returned, as_stored(s.clone()));
            let got = SummaryStore::get(&store, &summary_user("alice"), s.conversation_id)
                .await
                .unwrap();
            assert_eq!(got, Some(as_stored(s)));
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
            assert_eq!(got, Some(as_stored(s)));
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
            assert_eq!(sorted(got), vec![as_stored(a), as_stored(b)]);
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

        /// Replaces `put_again_replaces_the_earlier_summary` (plan
        /// 2026-10-06-load-only-what-the-page-shows.md §10b): a write with
        /// the version read replaces the record; a stale one is refused and
        /// changes nothing (§7, C1).
        #[tokio::test]
        async fn a_write_with_the_version_read_replaces_it_and_a_stale_one_is_refused() {
            let (store, _keep) = $make().await;
            let first = SummaryStore::put(&store, &summary_user("alice"), summary(1, "old", 1))
                .await
                .unwrap();
            assert_eq!(first.version, 1);
            // A second writer that read nothing (version 0) is refused.
            let stale_new =
                SummaryStore::put(&store, &summary_user("alice"), summary(1, "rival", 2)).await;
            assert!(
                matches!(
                    stale_new,
                    Err(timeline_core::ports::errors::StoreError::Conflict)
                ),
                "got {stale_new:?}"
            );
            let mut newer = first.clone();
            newer.name = SummaryConversationName("new".to_string());
            newer.message_count = 5;
            let second = SummaryStore::put(&store, &summary_user("alice"), newer.clone())
                .await
                .unwrap();
            assert_eq!(second, as_stored(newer));
            // Writing again from the first read is stale now.
            let stale = SummaryStore::put(&store, &summary_user("alice"), first).await;
            assert!(
                matches!(
                    stale,
                    Err(timeline_core::ports::errors::StoreError::Conflict)
                ),
                "got {stale:?}"
            );
            let got = store.list_for_user(&summary_user("alice")).await.unwrap();
            assert_eq!(got, vec![second]);
        }

        #[tokio::test]
        async fn a_page_of_records_starts_after_the_id_given_and_holds_at_most_max() {
            let (store, _keep) = $make().await;
            for n in [3, 1, 4, 2, 5] {
                SummaryStore::put(&store, &summary_user("alice"), summary(n, "c", 1))
                    .await
                    .unwrap();
            }
            SummaryStore::put(&store, &summary_user("bob"), summary(9, "b", 1))
                .await
                .unwrap();
            let ids = |list: Vec<SummaryRecord>| -> Vec<u128> {
                list.iter().map(|s| s.conversation_id.0.as_u128()).collect()
            };
            let first = store
                .list_page(&summary_user("alice"), None, 2)
                .await
                .unwrap();
            assert_eq!(ids(first.clone()), vec![1, 2]);
            let next = store
                .list_page(&summary_user("alice"), Some(first[1].conversation_id), 2)
                .await
                .unwrap();
            assert_eq!(ids(next.clone()), vec![3, 4]);
            let last = store
                .list_page(&summary_user("alice"), Some(next[1].conversation_id), 2)
                .await
                .unwrap();
            assert_eq!(ids(last), vec![5]);
        }
    };
}
