//! What any correct `SessionStore` must do, run against the in-memory fake
//! and `DynamoSessionStore` (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3, §6).
//!
//! `$make` is an async fn returning `(store, keep_alive)`.

macro_rules! session_contract {
    ($make:path) => {
        use timeline_core::model::ConversationId as SessConversationId;
        use timeline_core::ports::ids::UserId as SessUserId;
        use timeline_core::ports::sessions::SessionStore as SessStore;
        use timeline_core::stored_session::{
            Placement as SessPlacement, SessionCounts as SessCounts, SessionKey as SessKey,
            StoredSession as SessSession, ViewCounts as SessViewCounts,
        };

        fn sess_user(name: &str) -> SessUserId {
            SessUserId(name.to_string())
        }

        fn sess_conv(n: u128) -> SessConversationId {
            SessConversationId(uuid::Uuid::from_u128(n))
        }

        fn session(conv: u128, number: usize) -> SessSession {
            let start =
                chrono::DateTime::from_timestamp(1_700_000_000 + number as i64 * 7200, 0).unwrap();
            SessSession {
                conversation_id: sess_conv(conv),
                number,
                start,
                end: start + chrono::Duration::minutes(40),
                placement: SessPlacement::Gaps,
                message_count: 12,
                counts: SessCounts {
                    messages: 6,
                    reviewed: 2,
                    automatic: SessViewCounts {
                        caps: 1,
                        critical: 2,
                        angry: 0,
                        any: 3,
                    },
                    yours: SessViewCounts {
                        caps: 0,
                        critical: 1,
                        angry: 1,
                        any: 1,
                    },
                    both: SessViewCounts {
                        caps: 1,
                        critical: 1,
                        angry: 1,
                        any: 2,
                    },
                },
            }
        }

        #[tokio::test]
        async fn sessions_round_trip_and_list_in_key_order() {
            let (store, _keep) = $make().await;
            let mut placed = session(1, 0);
            placed.placement = SessPlacement::Span;
            // Written out of order, including a number past 9 and one past
            // 99, which must still sort as numbers.
            let written = vec![
                session(2, 0),
                session(1, 10),
                placed.clone(),
                session(1, 2),
                session(1, 100),
            ];
            store
                .put_sessions(&sess_user("alice"), &written)
                .await
                .unwrap();
            let got = store.list_sessions(&sess_user("alice")).await.unwrap();
            assert_eq!(
                got,
                vec![
                    placed,
                    session(1, 2),
                    session(1, 10),
                    session(1, 100),
                    session(2, 0)
                ]
            );
        }

        #[tokio::test]
        async fn a_page_starts_after_the_key_given_and_holds_at_most_max() {
            let (store, _keep) = $make().await;
            let all: Vec<SessSession> = (0..5).map(|n| session(1, n)).collect();
            store.put_sessions(&sess_user("alice"), &all).await.unwrap();
            let first = store
                .sessions_page(&sess_user("alice"), None, 2)
                .await
                .unwrap();
            assert_eq!(first, all[0..2].to_vec());
            let next = store
                .sessions_page(&sess_user("alice"), Some(first[1].key()), 2)
                .await
                .unwrap();
            assert_eq!(next, all[2..4].to_vec());
            let last = store
                .sessions_page(&sess_user("alice"), Some(next[1].key()), 2)
                .await
                .unwrap();
            assert_eq!(last, all[4..].to_vec());
            let none = store
                .sessions_page(&sess_user("alice"), Some(last[0].key()), 2)
                .await
                .unwrap();
            assert_eq!(none, vec![]);
        }

        #[tokio::test]
        async fn one_conversations_sessions_are_read_alone_in_number_order() {
            let (store, _keep) = $make().await;
            let written = vec![session(2, 1), session(1, 0), session(2, 0), session(3, 0)];
            store
                .put_sessions(&sess_user("alice"), &written)
                .await
                .unwrap();
            let got = store
                .sessions_of(&sess_user("alice"), sess_conv(2))
                .await
                .unwrap();
            assert_eq!(got, vec![session(2, 0), session(2, 1)]);
        }

        #[tokio::test]
        async fn writing_a_session_again_replaces_it() {
            let (store, _keep) = $make().await;
            store
                .put_sessions(&sess_user("alice"), &[session(1, 0)])
                .await
                .unwrap();
            let mut recounted = session(1, 0);
            recounted.counts.reviewed = 6;
            store
                .put_sessions(&sess_user("alice"), &[recounted.clone()])
                .await
                .unwrap();
            assert_eq!(
                store.list_sessions(&sess_user("alice")).await.unwrap(),
                vec![recounted]
            );
        }

        #[tokio::test]
        async fn deleting_sessions_removes_them_and_a_missing_one_is_no_error() {
            let (store, _keep) = $make().await;
            store
                .put_sessions(&sess_user("alice"), &[session(1, 0), session(1, 1)])
                .await
                .unwrap();
            store
                .delete_sessions(
                    &sess_user("alice"),
                    &[
                        SessKey {
                            conversation_id: sess_conv(1),
                            number: 1,
                        },
                        SessKey {
                            conversation_id: sess_conv(9),
                            number: 0,
                        },
                    ],
                )
                .await
                .unwrap();
            assert_eq!(
                store.list_sessions(&sess_user("alice")).await.unwrap(),
                vec![session(1, 0)]
            );
        }

        #[tokio::test]
        async fn one_users_sessions_are_invisible_to_another() {
            let (store, _keep) = $make().await;
            store
                .put_sessions(&sess_user("alice"), &[session(1, 0)])
                .await
                .unwrap();
            let bob = sess_user("bob");
            assert_eq!(store.list_sessions(&bob).await.unwrap(), vec![]);
            assert_eq!(store.sessions_page(&bob, None, 10).await.unwrap(), vec![]);
            assert_eq!(store.sessions_of(&bob, sess_conv(1)).await.unwrap(), vec![]);
            store
                .delete_sessions(&bob, &[session(1, 0).key()])
                .await
                .unwrap();
            assert_eq!(
                store.list_sessions(&sess_user("alice")).await.unwrap(),
                vec![session(1, 0)]
            );
        }

        #[tokio::test]
        async fn many_sessions_are_all_written_and_read() {
            let (store, _keep) = $make().await;
            let all: Vec<SessSession> = (0..60).map(|n| session(1, n)).collect();
            store.put_sessions(&sess_user("alice"), &all).await.unwrap();
            assert_eq!(store.list_sessions(&sess_user("alice")).await.unwrap(), all);
        }
    };
}
