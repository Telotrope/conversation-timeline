//! What any correct `UserRecordStore` and `AnalysisStore` must do, run
//! against the in-memory fake and `DynamoUserRecordStore` (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c, §8b).
//!
//! `$make` is an async fn returning `(store, keep_alive)`; the store must
//! implement both traits.

macro_rules! user_record_contract {
    ($make:path) => {
        use timeline_core::flag_view::FlagView as RecView;
        use timeline_core::ports::analyses::AnalysisStore as RecAnalyses;
        use timeline_core::ports::ids::UserId as RecUserId;
        use timeline_core::ports::user_record::{
            Totals as RecTotals, UserRecord as RecRecord, UserRecordStore as RecStore,
        };
        use timeline_core::server_analyses::{
            AnalysisRequest as RecRequest, SavedAnalysis as RecSaved,
            ServerAnalysis as RecAnalysis, TrendGranularity as RecGranularity,
            Unfinished as RecUnfinished,
        };
        use timeline_core::walk_cursor::WalkCursor as RecCursor;

        fn rec_user(name: &str) -> RecUserId {
            RecUserId(name.to_string())
        }

        fn totals(c: i64, s: i64, y: i64, m: i64) -> RecTotals {
            RecTotals {
                conversations: c,
                sessions: s,
                your_messages: y,
                messages: m,
            }
        }

        fn request(view: RecView) -> RecRequest {
            RecRequest {
                analysis: RecAnalysis::Trend(RecGranularity::Week),
                view,
                zone: chrono_tz::America::New_York,
            }
        }

        #[tokio::test]
        async fn a_user_who_stored_nothing_has_version_0_and_no_totals() {
            let (store, _keep) = $make().await;
            assert_eq!(
                RecStore::get(&store, &rec_user("alice")).await.unwrap(),
                RecRecord::default()
            );
        }

        #[tokio::test]
        async fn each_change_raises_the_version_and_adds_to_the_totals() {
            let (store, _keep) = $make().await;
            let first = store
                .record_change(&rec_user("alice"), totals(3, 5, 40, 80))
                .await
                .unwrap();
            assert_eq!(
                first,
                RecRecord {
                    data_version: 1,
                    totals: totals(3, 5, 40, 80)
                }
            );
            // A flag save changes no totals, and a branch revived later can
            // take some away.
            store
                .record_change(&rec_user("alice"), RecTotals::default())
                .await
                .unwrap();
            let third = store
                .record_change(&rec_user("alice"), totals(1, -2, -1, -3))
                .await
                .unwrap();
            assert_eq!(
                third,
                RecRecord {
                    data_version: 3,
                    totals: totals(4, 3, 39, 77)
                }
            );
            assert_eq!(
                RecStore::get(&store, &rec_user("alice")).await.unwrap(),
                third
            );
        }

        #[tokio::test]
        async fn one_users_record_is_invisible_to_another() {
            let (store, _keep) = $make().await;
            store
                .record_change(&rec_user("alice"), totals(1, 1, 1, 1))
                .await
                .unwrap();
            assert_eq!(
                RecStore::get(&store, &rec_user("bob")).await.unwrap(),
                RecRecord::default()
            );
        }

        #[tokio::test]
        async fn a_saved_analysis_round_trips_and_is_replaced_by_a_later_one() {
            let (store, _keep) = $make().await;
            let key = request(RecView::Both).key();
            assert_eq!(
                RecAnalyses::get(&store, &rec_user("alice"), &key)
                    .await
                    .unwrap(),
                None
            );
            let unfinished = RecSaved {
                data_version: 4,
                numbers: request(RecView::Both).empty(),
                unfinished: Some(RecUnfinished {
                    cursor: RecCursor {
                        group: timeline_core::stored_session::SessionKey {
                            conversation_id: timeline_core::model::ConversationId(
                                uuid::Uuid::from_u128(1),
                            ),
                            number: 3,
                        },
                        after: None,
                    },
                    sessions_done: 7,
                }),
            };
            RecAnalyses::put(&store, &rec_user("alice"), &key, &unfinished)
                .await
                .unwrap();
            assert_eq!(
                RecAnalyses::get(&store, &rec_user("alice"), &key)
                    .await
                    .unwrap(),
                Some(unfinished.clone())
            );
            let done = RecSaved {
                unfinished: None,
                ..unfinished
            };
            RecAnalyses::put(&store, &rec_user("alice"), &key, &done)
                .await
                .unwrap();
            assert_eq!(
                RecAnalyses::get(&store, &rec_user("alice"), &key)
                    .await
                    .unwrap(),
                Some(done)
            );
        }

        #[tokio::test]
        async fn analyses_are_kept_apart_by_options_and_by_user() {
            let (store, _keep) = $make().await;
            let saved = RecSaved {
                data_version: 1,
                numbers: request(RecView::Both).empty(),
                unfinished: None,
            };
            RecAnalyses::put(
                &store,
                &rec_user("alice"),
                &request(RecView::Both).key(),
                &saved,
            )
            .await
            .unwrap();
            assert_eq!(
                RecAnalyses::get(&store, &rec_user("alice"), &request(RecView::Yours).key())
                    .await
                    .unwrap(),
                None
            );
            assert_eq!(
                RecAnalyses::get(&store, &rec_user("bob"), &request(RecView::Both).key())
                    .await
                    .unwrap(),
                None
            );
        }
    };
}
