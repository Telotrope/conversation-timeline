//! What any correct message-flags store must do, run against the in-memory
//! fake and `DynamoMessageFlagsStore`. See the migration plan's §V2b.
//!
//! The auto/user separation (plan §4.1) is checked by reading back what is
//! stored after each kind of write, through the real trait methods -- not
//! by inspecting the query text. This is the public-API proof that
//! `dynamo::message_flags_table`'s `auto_update_expression` and
//! `user_update_expression` only ever touch their own attributes.
//!
//! `$make` is an async fn returning `(store, keep_alive)`; the store must
//! implement all three flag traits.

macro_rules! message_flags_contract {
    ($make:path) => {
        use timeline_core::model::{
            ConversationId as FlagsConversationId, MessageId as FlagsMessageId,
        };
        use timeline_core::ports::ids::UserId as FlagsUserId;
        use timeline_core::ports::message_flags::{
            AutoFlagWriter as FlagsAutoWriter, FlagOverrides as FlagsOverrides,
            FlagSet as FlagsSet, MessageFlagRecord as FlagsRecord,
            MessageFlagsReader as FlagsReader, UserFlagWriter as FlagsUserWriter,
        };

        fn flags_user() -> FlagsUserId {
            FlagsUserId("alice".to_string())
        }
        fn flags_conv() -> FlagsConversationId {
            FlagsConversationId(uuid::Uuid::from_u128(1))
        }
        fn flags_msg() -> FlagsMessageId {
            FlagsMessageId(uuid::Uuid::from_u128(2))
        }

        #[tokio::test]
        async fn get_before_any_write_is_none() {
            let (store, _keep) = $make().await;
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap();
            assert_eq!(got, None);
        }

        #[tokio::test]
        async fn an_auto_write_sets_no_user_override() {
            let (store, _keep) = $make().await;
            let auto = FlagsSet {
                caps: true,
                critical: false,
                angry: true,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), flags_msg(), auto)
                .await
                .unwrap();
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap();
            assert_eq!(
                got,
                Some(FlagsRecord {
                    message_id: flags_msg(),
                    auto,
                    user: FlagsOverrides::default()
                })
            );
        }

        #[tokio::test]
        async fn a_second_auto_write_replaces_the_first_and_keeps_user_overrides() {
            let (store, _keep) = $make().await;
            let overrides = FlagsOverrides {
                caps: Some(false),
                critical: None,
                angry: Some(true),
            };
            store
                .set_user_flags(&flags_user(), flags_conv(), flags_msg(), overrides)
                .await
                .unwrap();
            store
                .set_auto_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsSet {
                        caps: true,
                        critical: true,
                        angry: true,
                    },
                )
                .await
                .unwrap();
            let second = FlagsSet {
                caps: false,
                critical: true,
                angry: false,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), flags_msg(), second)
                .await
                .unwrap();
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(got.auto, second);
            assert_eq!(
                got.user, overrides,
                "an auto write must never change user overrides"
            );
        }

        #[tokio::test]
        async fn a_user_write_does_not_disturb_auto_flags() {
            let (store, _keep) = $make().await;
            let auto = FlagsSet {
                caps: true,
                critical: true,
                angry: false,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), flags_msg(), auto)
                .await
                .unwrap();
            let overrides = FlagsOverrides {
                caps: Some(false),
                critical: None,
                angry: None,
            };
            store
                .set_user_flags(&flags_user(), flags_conv(), flags_msg(), overrides)
                .await
                .unwrap();
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                got.auto, auto,
                "a user write must never change the auto flags"
            );
            assert_eq!(got.user, overrides);
        }

        #[tokio::test]
        async fn set_user_flags_returns_the_record_as_stored() {
            let (store, _keep) = $make().await;
            let auto = FlagsSet {
                caps: false,
                critical: true,
                angry: false,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), flags_msg(), auto)
                .await
                .unwrap();
            let returned = store
                .set_user_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsOverrides {
                        caps: None,
                        critical: Some(false),
                        angry: None,
                    },
                )
                .await
                .unwrap();
            let stored = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap();
            assert_eq!(Some(returned), stored);
        }

        #[tokio::test]
        async fn a_user_write_on_a_message_with_no_record_creates_one_with_no_auto_flags() {
            let (store, _keep) = $make().await;
            let overrides = FlagsOverrides {
                caps: Some(true),
                critical: Some(false),
                angry: Some(true),
            };
            let returned = store
                .set_user_flags(&flags_user(), flags_conv(), flags_msg(), overrides)
                .await
                .unwrap();
            assert_eq!(
                returned,
                FlagsRecord {
                    message_id: flags_msg(),
                    auto: FlagsSet::default(),
                    user: overrides
                }
            );
        }

        #[tokio::test]
        async fn a_partial_user_update_only_touches_the_flags_it_names() {
            let (store, _keep) = $make().await;
            store
                .set_user_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsOverrides {
                        caps: Some(true),
                        critical: None,
                        angry: None,
                    },
                )
                .await
                .unwrap();
            store
                .set_user_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsOverrides {
                        caps: None,
                        critical: Some(true),
                        angry: None,
                    },
                )
                .await
                .unwrap();
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                got.user,
                FlagsOverrides {
                    caps: Some(true),
                    critical: Some(true),
                    angry: None
                }
            );
        }

        #[tokio::test]
        async fn an_empty_user_update_on_an_existing_record_changes_nothing() {
            let (store, _keep) = $make().await;
            let auto = FlagsSet {
                caps: true,
                critical: false,
                angry: false,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), flags_msg(), auto)
                .await
                .unwrap();
            let before = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap();
            let returned = store
                .set_user_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsOverrides::default(),
                )
                .await
                .unwrap();
            assert_eq!(Some(returned), before);
        }

        /// Both stores must agree here: an empty update on a message with no
        /// record is NotFound and creates nothing (migration plan §V2c).
        #[tokio::test]
        async fn an_empty_user_update_on_a_message_with_no_record_is_not_found_and_creates_nothing()
        {
            let (store, _keep) = $make().await;
            let result = store
                .set_user_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsOverrides::default(),
                )
                .await;
            assert!(
                matches!(
                    result,
                    Err(timeline_core::ports::errors::StoreError::NotFound)
                ),
                "expected NotFound, got {result:?}"
            );
            let got = FlagsReader::get(&store, &flags_user(), flags_conv(), flags_msg())
                .await
                .unwrap();
            assert_eq!(got, None, "an empty update must not create a record");
        }

        #[tokio::test]
        async fn list_for_conversation_returns_every_message_in_it() {
            let (store, _keep) = $make().await;
            let msg_a = FlagsMessageId(uuid::Uuid::from_u128(10));
            let msg_b = FlagsMessageId(uuid::Uuid::from_u128(11));
            let auto_b = FlagsSet {
                caps: false,
                critical: false,
                angry: true,
            };
            store
                .set_auto_flags(&flags_user(), flags_conv(), msg_a, FlagsSet::default())
                .await
                .unwrap();
            store
                .set_auto_flags(&flags_user(), flags_conv(), msg_b, auto_b)
                .await
                .unwrap();
            let mut got = FlagsReader::list_for_conversation(&store, &flags_user(), flags_conv())
                .await
                .unwrap();
            got.sort_by_key(|r| r.message_id.0);
            assert_eq!(
                got,
                vec![
                    FlagsRecord {
                        message_id: msg_a,
                        auto: FlagsSet::default(),
                        user: FlagsOverrides::default()
                    },
                    FlagsRecord {
                        message_id: msg_b,
                        auto: auto_b,
                        user: FlagsOverrides::default()
                    },
                ]
            );
        }

        #[tokio::test]
        async fn list_for_conversation_excludes_other_conversations() {
            let (store, _keep) = $make().await;
            let other_conv = FlagsConversationId(uuid::Uuid::from_u128(999));
            store
                .set_auto_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsSet::default(),
                )
                .await
                .unwrap();
            store
                .set_auto_flags(&flags_user(), other_conv, flags_msg(), FlagsSet::default())
                .await
                .unwrap();
            let got = FlagsReader::list_for_conversation(&store, &flags_user(), flags_conv())
                .await
                .unwrap();
            assert_eq!(got.len(), 1);
        }

        #[tokio::test]
        async fn flags_are_isolated_per_user() {
            let (store, _keep) = $make().await;
            store
                .set_auto_flags(
                    &flags_user(),
                    flags_conv(),
                    flags_msg(),
                    FlagsSet {
                        caps: true,
                        critical: true,
                        angry: true,
                    },
                )
                .await
                .unwrap();
            let bob = FlagsUserId("bob".to_string());
            let got = FlagsReader::get(&store, &bob, flags_conv(), flags_msg())
                .await
                .unwrap();
            assert_eq!(got, None);
            let listed = FlagsReader::list_for_conversation(&store, &bob, flags_conv())
                .await
                .unwrap();
            assert!(
                listed.is_empty(),
                "bob must not see alice's flags: {listed:?}"
            );
        }
    };
}
