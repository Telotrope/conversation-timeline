//! What any correct message-row store must do, run against the in-memory
//! fake and `DynamoMessageStore` (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3). Replaces
//! the `MessageFlags` contract: flags now live on the message rows. Each
//! test that replaces one of that contract's tests names it.
//!
//! The automatic/yours separation (migration plan §4.1) is checked by
//! reading back what is stored after each kind of write, through the real
//! trait methods, not by inspecting the update's text.
//!
//! `$make` is an async fn returning `(store, keep_alive)`; the store must
//! implement every message trait.

macro_rules! message_rows_contract {
    ($make:path) => {
        use timeline_core::flag_values::{
            FlagOverrides as RowsOverrides, FlagSet as RowsFlagSet, MessageFlags as RowsFlags,
        };
        use timeline_core::model::{
            ConversationId as RowsConversationId, MessageId as RowsMessageId, Sender as RowsSender,
        };
        use timeline_core::ports::errors::StoreError as RowsStoreError;
        use timeline_core::ports::ids::UserId as RowsUserId;
        use timeline_core::ports::messages::{
            AutoFlagWriter as RowsAutoWriter, EntryRange as RowsRange, MessageReader as RowsReader,
            MessageRowWriter as RowsWriter, UserFlagWriter as RowsUserWriter,
        };
        use timeline_core::stored_message::{
            BranchNote as RowsNote, Citation as RowsCitation, CitedAddress as RowsAddress,
            Entry as RowsEntry, EntryKey as RowsKey, Piece as RowsPiece,
            StoredMessage as RowsMessage,
        };

        fn rows_user(name: &str) -> RowsUserId {
            RowsUserId(name.to_string())
        }

        fn rows_conv(n: u128) -> RowsConversationId {
            RowsConversationId(uuid::Uuid::from_u128(n))
        }

        fn rows_at(minute: i64) -> chrono::DateTime<chrono::Utc> {
            chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0).unwrap()
        }

        fn rows_key(conv: u128, minute: i64, id: u128) -> RowsKey {
            RowsKey {
                conversation_id: rows_conv(conv),
                at: rows_at(minute),
                id: RowsMessageId(uuid::Uuid::from_u128(id)),
            }
        }

        /// Your message, with flags as processing writes them.
        fn yours(key: RowsKey, flags: RowsFlags) -> RowsEntry {
            RowsEntry::Message(RowsMessage {
                key,
                parent: None,
                sender: RowsSender::Human,
                pieces: vec![RowsPiece::Text {
                    text: "Some “text” — ünïcode".to_string(),
                    citations: Vec::new(),
                }],
                attachments: Vec::new(),
                flags: Some(flags),
            })
        }

        fn claudes(key: RowsKey) -> RowsEntry {
            RowsEntry::Message(RowsMessage {
                key,
                parent: Some(RowsMessageId(uuid::Uuid::from_u128(999))),
                sender: RowsSender::Assistant,
                pieces: vec![RowsPiece::Text {
                    text: "A reply with a source.".to_string(),
                    citations: vec![RowsCitation {
                        start: 2,
                        end: 7,
                        address: RowsAddress::parse("https://example.com/a"),
                    }],
                }],
                attachments: Vec::new(),
                flags: None,
            })
        }

        fn note(key: RowsKey) -> RowsEntry {
            RowsEntry::Note(RowsNote {
                key,
                last_at: key.at,
                messages: 2,
                words_not_repeated: 0,
                replaced_by: None,
                kept_as: None,
            })
        }

        fn review(
            caps: Option<bool>,
            critical: Option<bool>,
            angry: Option<bool>,
        ) -> RowsOverrides {
            RowsOverrides {
                caps,
                critical,
                angry,
            }
        }

        fn flags_of(entry: Option<RowsEntry>) -> Option<RowsFlags> {
            match entry {
                Some(RowsEntry::Message(m)) => m.flags,
                _ => None,
            }
        }

        async fn stored_flags<S: RowsReader>(store: &S, key: RowsKey) -> Option<RowsFlags> {
            flags_of(
                store
                    .find_entry(&rows_user("alice"), key.conversation_id, key.id)
                    .await
                    .unwrap(),
            )
        }

        fn whole(conv: u128) -> RowsRange {
            RowsRange {
                conversation_id: rows_conv(conv),
                times: None,
                after: None,
            }
        }

        /// Replaces `message_flags_contract.rs`'s `get_before_any_write_is_none`.
        #[tokio::test]
        async fn reading_a_message_that_was_never_written_is_none() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            let got = store
                .find_entry(&rows_user("alice"), key.conversation_id, key.id)
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        /// Replaces `memory_message_flags.rs`'s `get_before_any_write_is_none`.
        #[tokio::test]
        async fn a_stored_message_has_no_automatic_flags_and_no_review() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            assert_eq!(stored_flags(&store, key).await, Some(RowsFlags::default()));
        }

        #[tokio::test]
        async fn every_kind_of_entry_round_trips() {
            let (store, _keep) = $make().await;
            let reviewed = RowsFlags {
                auto: Some(RowsFlagSet {
                    caps: true,
                    critical: false,
                    angry: true,
                }),
                user: review(Some(false), None, Some(true)),
            };
            let entries = vec![
                yours(rows_key(1, 0, 2), reviewed),
                claudes(rows_key(1, 1, 3)),
                note(rows_key(1, 2, 4)),
            ];
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            let got = store
                .read_entries(&rows_user("alice"), whole(1))
                .await
                .unwrap();
            assert_eq!(got, entries);
        }

        #[tokio::test]
        async fn a_message_of_unknown_time_round_trips_and_sorts_first() {
            let (store, _keep) = $make().await;
            let untimed = RowsKey {
                at: timeline_core::UNKNOWN_TIME,
                ..rows_key(1, 0, 9)
            };
            let entries = vec![
                yours(rows_key(1, 5, 2), RowsFlags::default()),
                yours(untimed, RowsFlags::default()),
            ];
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            let got = store
                .read_entries(&rows_user("alice"), whole(1))
                .await
                .unwrap();
            assert_eq!(got, vec![entries[1].clone(), entries[0].clone()]);
            assert_eq!(got[0].key().time(), timeline_core::MessageTime::Unknown);
        }

        #[tokio::test]
        async fn writing_a_row_again_replaces_it() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            let again = RowsFlags {
                auto: None,
                user: review(Some(true), None, None),
            };
            store
                .put_entries(&rows_user("alice"), &[yours(key, again)])
                .await
                .unwrap();
            let got = store
                .read_entries(&rows_user("alice"), whole(1))
                .await
                .unwrap();
            assert_eq!(got, vec![yours(key, again)]);
        }

        /// Replaces `message_flags_contract.rs`'s `an_auto_write_sets_no_user_override`
        /// and `memory_message_flags.rs`'s
        /// `auto_write_is_visible_via_read_with_no_user_overrides_set`.
        #[tokio::test]
        async fn an_automatic_write_is_read_back_with_no_review() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            let auto = RowsFlagSet {
                caps: true,
                critical: false,
                angry: true,
            };
            store
                .set_auto_flags(&rows_user("alice"), key, auto)
                .await
                .unwrap();
            assert_eq!(
                stored_flags(&store, key).await,
                Some(RowsFlags {
                    auto: Some(auto),
                    user: RowsOverrides::default()
                })
            );
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `a_second_auto_write_replaces_the_first_and_keeps_user_overrides`.
        #[tokio::test]
        async fn a_second_scan_replaces_the_first_and_keeps_the_review() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            let mine = review(Some(false), None, Some(true));
            store
                .set_user_flags(&rows_user("alice"), key, mine)
                .await
                .unwrap();
            let first = RowsFlagSet {
                caps: true,
                critical: true,
                angry: true,
            };
            let second = RowsFlagSet {
                caps: false,
                critical: true,
                angry: false,
            };
            store
                .set_auto_flags(&rows_user("alice"), key, first)
                .await
                .unwrap();
            store
                .set_auto_flags(&rows_user("alice"), key, second)
                .await
                .unwrap();
            assert_eq!(
                stored_flags(&store, key).await,
                Some(RowsFlags {
                    auto: Some(second),
                    user: mine
                })
            );
        }

        /// Replaces `message_flags_contract.rs`'s `a_user_write_does_not_disturb_auto_flags`
        /// and `memory_message_flags.rs`'s
        /// `user_write_is_visible_via_read_without_disturbing_auto`.
        #[tokio::test]
        async fn a_review_write_leaves_the_automatic_flags_alone() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            let auto = RowsFlagSet {
                caps: true,
                critical: true,
                angry: false,
            };
            store
                .set_auto_flags(&rows_user("alice"), key, auto)
                .await
                .unwrap();
            let mine = review(Some(false), Some(false), Some(true));
            store
                .set_user_flags(&rows_user("alice"), key, mine)
                .await
                .unwrap();
            assert_eq!(
                stored_flags(&store, key).await,
                Some(RowsFlags {
                    auto: Some(auto),
                    user: mine
                })
            );
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `set_user_flags_returns_the_record_as_stored`.
        #[tokio::test]
        async fn a_review_write_returns_the_flags_as_stored() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            let auto = RowsFlagSet {
                caps: true,
                critical: false,
                angry: false,
            };
            store
                .put_entries(
                    &rows_user("alice"),
                    &[yours(
                        key,
                        RowsFlags {
                            auto: Some(auto),
                            user: review(None, Some(true), None),
                        },
                    )],
                )
                .await
                .unwrap();
            let returned = store
                .set_user_flags(&rows_user("alice"), key, review(Some(false), None, None))
                .await
                .unwrap();
            assert_eq!(
                returned,
                RowsFlags {
                    auto: Some(auto),
                    user: review(Some(false), Some(true), None)
                }
            );
            assert_eq!(stored_flags(&store, key).await, Some(returned));
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `a_user_write_on_a_message_with_no_record_creates_one_with_no_auto_flags`:
        /// every message has a row now, so a write without one names a
        /// message that isn't stored.
        #[tokio::test]
        async fn a_review_write_on_a_message_with_no_row_is_not_found_and_creates_nothing() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            let got = store
                .set_user_flags(&rows_user("alice"), key, review(Some(true), None, None))
                .await;
            assert!(matches!(got, Err(RowsStoreError::NotFound)), "got {got:?}");
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![]
            );
        }

        #[tokio::test]
        async fn an_automatic_write_on_a_message_with_no_row_is_not_found_and_creates_nothing() {
            let (store, _keep) = $make().await;
            let got = store
                .set_auto_flags(
                    &rows_user("alice"),
                    rows_key(1, 0, 2),
                    RowsFlagSet::default(),
                )
                .await;
            assert!(matches!(got, Err(RowsStoreError::NotFound)), "got {got:?}");
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![]
            );
        }

        /// Claude's messages and notes carry no flags: flag writes to them
        /// are refused like writes to a row that isn't there, and change
        /// nothing.
        #[tokio::test]
        async fn flags_cannot_be_written_onto_claudes_messages_or_notes() {
            let (store, _keep) = $make().await;
            let entries = vec![claudes(rows_key(1, 0, 2)), note(rows_key(1, 1, 3))];
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            for entry in &entries {
                let auto = store
                    .set_auto_flags(&rows_user("alice"), entry.key(), RowsFlagSet::default())
                    .await;
                assert!(
                    matches!(auto, Err(RowsStoreError::NotFound)),
                    "got {auto:?}"
                );
                let user = store
                    .set_user_flags(
                        &rows_user("alice"),
                        entry.key(),
                        review(Some(true), None, None),
                    )
                    .await;
                assert!(
                    matches!(user, Err(RowsStoreError::NotFound)),
                    "got {user:?}"
                );
                let empty = store
                    .set_user_flags(&rows_user("alice"), entry.key(), RowsOverrides::default())
                    .await;
                assert!(
                    matches!(empty, Err(RowsStoreError::NotFound)),
                    "got {empty:?}"
                );
            }
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                entries
            );
        }

        /// Replaces `message_flags_contract.rs`'s and `memory_message_flags.rs`'s
        /// `a_partial_user_update_only_touches_the_flags_it_names`.
        #[tokio::test]
        async fn a_partial_review_changes_only_the_flags_it_names() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            store
                .set_user_flags(
                    &rows_user("alice"),
                    key,
                    review(Some(true), Some(false), Some(true)),
                )
                .await
                .unwrap();
            store
                .set_user_flags(&rows_user("alice"), key, review(None, Some(true), None))
                .await
                .unwrap();
            assert_eq!(
                stored_flags(&store, key).await.unwrap().user,
                review(Some(true), Some(true), Some(true))
            );
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `an_empty_user_update_on_an_existing_record_changes_nothing`.
        #[tokio::test]
        async fn an_empty_review_changes_nothing() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            let before = RowsFlags {
                auto: Some(RowsFlagSet {
                    caps: false,
                    critical: true,
                    angry: false,
                }),
                user: review(Some(true), None, None),
            };
            store
                .put_entries(&rows_user("alice"), &[yours(key, before)])
                .await
                .unwrap();
            let returned = store
                .set_user_flags(&rows_user("alice"), key, RowsOverrides::default())
                .await
                .unwrap();
            assert_eq!(returned, before);
            assert_eq!(stored_flags(&store, key).await, Some(before));
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `an_empty_user_update_on_a_message_with_no_record_is_not_found_and_creates_nothing`.
        #[tokio::test]
        async fn an_empty_review_on_a_message_with_no_row_is_not_found_and_creates_nothing() {
            let (store, _keep) = $make().await;
            let got = store
                .set_user_flags(
                    &rows_user("alice"),
                    rows_key(1, 0, 2),
                    RowsOverrides::default(),
                )
                .await;
            assert!(matches!(got, Err(RowsStoreError::NotFound)), "got {got:?}");
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![]
            );
        }

        /// Replaces `memory_message_flags.rs`'s
        /// `list_for_conversation_returns_every_message_in_it`.
        #[tokio::test]
        async fn reading_a_conversation_returns_every_entry_in_it_in_time_order() {
            let (store, _keep) = $make().await;
            let late = yours(rows_key(1, 30, 2), RowsFlags::default());
            let early = claudes(rows_key(1, 1, 3));
            let middle = note(rows_key(1, 10, 4));
            store
                .put_entries(
                    &rows_user("alice"),
                    &[late.clone(), early.clone(), middle.clone()],
                )
                .await
                .unwrap();
            let got = store
                .read_entries(&rows_user("alice"), whole(1))
                .await
                .unwrap();
            assert_eq!(got, vec![early, middle, late]);
        }

        /// Replaces `memory_message_flags.rs`'s
        /// `list_for_conversation_does_not_include_a_different_conversations_messages`
        /// and `message_flags_contract.rs`'s `list_for_conversation_excludes_other_conversations`.
        #[tokio::test]
        async fn reading_a_conversation_or_a_span_leaves_out_other_conversations() {
            let (store, _keep) = $make().await;
            let mine = yours(rows_key(1, 5, 2), RowsFlags::default());
            // Another conversation, at the same time and just after.
            let other = yours(rows_key(2, 5, 3), RowsFlags::default());
            store
                .put_entries(&rows_user("alice"), &[mine.clone(), other])
                .await
                .unwrap();
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![mine.clone()]
            );
            let span = RowsRange {
                times: Some((rows_at(0), rows_at(10))),
                ..whole(1)
            };
            assert_eq!(
                store.read_entries(&rows_user("alice"), span).await.unwrap(),
                vec![mine]
            );
        }

        /// Replaces `message_flags_contract.rs`'s
        /// `list_for_conversation_returns_every_message_in_it`: a session's
        /// rows are read by its span, ends included.
        #[tokio::test]
        async fn reading_a_span_returns_exactly_its_entries_ends_included() {
            let (store, _keep) = $make().await;
            let before = yours(rows_key(1, 0, 2), RowsFlags::default());
            let first = yours(rows_key(1, 20, 3), RowsFlags::default());
            let inside = claudes(rows_key(1, 25, 4));
            let last = yours(rows_key(1, 30, 5), RowsFlags::default());
            let after = yours(rows_key(1, 50, 6), RowsFlags::default());
            store
                .put_entries(
                    &rows_user("alice"),
                    &[before, first.clone(), inside.clone(), last.clone(), after],
                )
                .await
                .unwrap();
            let span = RowsRange {
                times: Some((rows_at(20), rows_at(30))),
                ..whole(1)
            };
            assert_eq!(
                store.read_entries(&rows_user("alice"), span).await.unwrap(),
                vec![first, inside, last]
            );
        }

        /// Where an earlier part stopped (plan §8c): reading after a key
        /// carries on from the next row.
        #[tokio::test]
        async fn reading_after_a_key_carries_on_from_the_next_row() {
            let (store, _keep) = $make().await;
            let entries: Vec<RowsEntry> = (0..5)
                .map(|i| yours(rows_key(1, i, 10 + i as u128), RowsFlags::default()))
                .collect();
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            let range = RowsRange {
                after: Some(entries[1].key()),
                ..whole(1)
            };
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), range)
                    .await
                    .unwrap(),
                entries[2..].to_vec()
            );
            let span_after = RowsRange {
                times: Some((rows_at(0), rows_at(3))),
                after: Some(entries[2].key()),
                ..whole(1)
            };
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), span_after)
                    .await
                    .unwrap(),
                vec![entries[3].clone()]
            );
        }

        #[tokio::test]
        async fn the_entry_after_a_message_is_the_next_in_its_conversation_only() {
            let (store, _keep) = $make().await;
            let question = yours(rows_key(1, 0, 2), RowsFlags::default());
            let reply = claudes(rows_key(1, 1, 3));
            let elsewhere = claudes(rows_key(2, 0, 4));
            store
                .put_entries(
                    &rows_user("alice"),
                    &[question.clone(), reply.clone(), elsewhere],
                )
                .await
                .unwrap();
            assert_eq!(
                store
                    .entry_after(&rows_user("alice"), question.key())
                    .await
                    .unwrap(),
                Some(reply.clone())
            );
            assert_eq!(
                store
                    .entry_after(&rows_user("alice"), reply.key())
                    .await
                    .unwrap(),
                None
            );
        }

        #[tokio::test]
        async fn finding_a_message_by_id_reads_it_whatever_its_time() {
            let (store, _keep) = $make().await;
            let entries = vec![
                yours(rows_key(1, 0, 2), RowsFlags::default()),
                claudes(rows_key(1, 600, 3)),
            ];
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            let got = store
                .find_entry(
                    &rows_user("alice"),
                    rows_conv(1),
                    RowsMessageId(uuid::Uuid::from_u128(3)),
                )
                .await
                .unwrap();
            assert_eq!(got, Some(entries[1].clone()));
            let wrong_conversation = store
                .find_entry(
                    &rows_user("alice"),
                    rows_conv(2),
                    RowsMessageId(uuid::Uuid::from_u128(3)),
                )
                .await
                .unwrap();
            assert_eq!(wrong_conversation, None);
        }

        #[tokio::test]
        async fn finding_by_id_never_returns_a_note() {
            let (store, _keep) = $make().await;
            store
                .put_entries(&rows_user("alice"), &[note(rows_key(1, 0, 2))])
                .await
                .unwrap();
            let got = store
                .find_entry(
                    &rows_user("alice"),
                    rows_conv(1),
                    RowsMessageId(uuid::Uuid::from_u128(2)),
                )
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn deleting_rows_removes_them_and_a_missing_one_is_no_error() {
            let (store, _keep) = $make().await;
            let keep = yours(rows_key(1, 0, 2), RowsFlags::default());
            let gone = claudes(rows_key(1, 1, 3));
            store
                .put_entries(&rows_user("alice"), &[keep.clone(), gone.clone()])
                .await
                .unwrap();
            store
                .delete_entries(&rows_user("alice"), &[gone.key(), rows_key(9, 9, 9)])
                .await
                .unwrap();
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![keep]
            );
        }

        /// Replaces `message_flags_contract.rs`'s and `memory_message_flags.rs`'s
        /// `flags_are_isolated_per_user`.
        #[tokio::test]
        async fn one_users_rows_are_invisible_to_another() {
            let (store, _keep) = $make().await;
            let key = rows_key(1, 0, 2);
            store
                .put_entries(&rows_user("alice"), &[yours(key, RowsFlags::default())])
                .await
                .unwrap();
            let bob = rows_user("bob");
            assert_eq!(store.read_entries(&bob, whole(1)).await.unwrap(), vec![]);
            assert_eq!(
                store
                    .find_entry(&bob, key.conversation_id, key.id)
                    .await
                    .unwrap(),
                None
            );
            assert!(matches!(
                store
                    .set_auto_flags(&bob, key, RowsFlagSet::default())
                    .await,
                Err(RowsStoreError::NotFound)
            ));
            assert!(matches!(
                store
                    .set_user_flags(&bob, key, review(Some(true), None, None))
                    .await,
                Err(RowsStoreError::NotFound)
            ));
            store.delete_entries(&bob, &[key]).await.unwrap();
            assert_eq!(stored_flags(&store, key).await, Some(RowsFlags::default()));
        }

        /// More rows than DynamoDB writes in one batch (25), written and read
        /// back whole.
        #[tokio::test]
        async fn many_rows_are_all_written_and_read() {
            let (store, _keep) = $make().await;
            let entries: Vec<RowsEntry> = (0..120)
                .map(|i| yours(rows_key(1, i, 1000 + i as u128), RowsFlags::default()))
                .collect();
            store
                .put_entries(&rows_user("alice"), &entries)
                .await
                .unwrap();
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                entries
            );
            let keys: Vec<RowsKey> = entries.iter().map(RowsEntry::key).collect();
            store
                .delete_entries(&rows_user("alice"), &keys)
                .await
                .unwrap();
            assert_eq!(
                store
                    .read_entries(&rows_user("alice"), whole(1))
                    .await
                    .unwrap(),
                vec![]
            );
        }
    };
}
