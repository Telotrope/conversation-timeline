//! Black-box tests for `InMemoryMessageFlagsStore`, including proving the
//! auto/user separation through the real public trait methods -- not just
//! at the DynamoDB-expression level (see
//! `timeline-storage/src/dynamo/message_flags_table.rs`'s temporary
//! private-function tests for that).

use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::ids::UserId;
use timeline_core::ports::message_flags::{
    AutoFlagWriter, FlagOverrides, FlagSet, MessageFlagsReader, UserFlagWriter,
};
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;

fn user() -> UserId {
    UserId("alice".to_string())
}
fn conv() -> ConversationId {
    ConversationId(uuid::Uuid::from_u128(1))
}
fn msg() -> MessageId {
    MessageId(uuid::Uuid::from_u128(2))
}

#[tokio::test]
async fn get_before_any_write_is_none() {
    let store = InMemoryMessageFlagsStore::new();
    assert!(MessageFlagsReader::get(&store, &user(), conv(), msg())
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn auto_write_is_visible_via_read_with_no_user_overrides_set() {
    let store = InMemoryMessageFlagsStore::new();
    store
        .set_auto_flags(
            &user(),
            conv(),
            msg(),
            FlagSet {
                caps: true,
                critical: false,
                angry: true,
            },
        )
        .await
        .unwrap();

    let record = MessageFlagsReader::get(&store, &user(), conv(), msg())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.auto,
        FlagSet {
            caps: true,
            critical: false,
            angry: true
        }
    );
    assert_eq!(
        record.user,
        FlagOverrides::default(),
        "a pure auto write must never set any user override"
    );
}

#[tokio::test]
async fn user_write_is_visible_via_read_without_disturbing_auto() {
    let store = InMemoryMessageFlagsStore::new();
    store
        .set_auto_flags(
            &user(),
            conv(),
            msg(),
            FlagSet {
                caps: true,
                critical: true,
                angry: false,
            },
        )
        .await
        .unwrap();

    store
        .set_user_flags(
            &user(),
            conv(),
            msg(),
            FlagOverrides {
                caps: Some(false),
                critical: None,
                angry: None,
            },
        )
        .await
        .unwrap();

    let record = MessageFlagsReader::get(&store, &user(), conv(), msg())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.auto,
        FlagSet {
            caps: true,
            critical: true,
            angry: false
        },
        "a pure user write must never change the auto flags"
    );
    assert_eq!(
        record.user,
        FlagOverrides {
            caps: Some(false),
            critical: None,
            angry: None
        }
    );
}

#[tokio::test]
async fn a_partial_user_update_only_touches_the_flags_it_names() {
    let store = InMemoryMessageFlagsStore::new();
    store
        .set_user_flags(
            &user(),
            conv(),
            msg(),
            FlagOverrides {
                caps: Some(true),
                critical: None,
                angry: None,
            },
        )
        .await
        .unwrap();
    store
        .set_user_flags(
            &user(),
            conv(),
            msg(),
            FlagOverrides {
                caps: None,
                critical: Some(true),
                angry: None,
            },
        )
        .await
        .unwrap();

    let record = MessageFlagsReader::get(&store, &user(), conv(), msg())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.user,
        FlagOverrides {
            caps: Some(true),
            critical: Some(true),
            angry: None
        }
    );
}

#[tokio::test]
async fn list_for_conversation_returns_every_message_in_it() {
    let store = InMemoryMessageFlagsStore::new();
    let msg_a = MessageId(uuid::Uuid::from_u128(10));
    let msg_b = MessageId(uuid::Uuid::from_u128(11));
    store
        .set_auto_flags(&user(), conv(), msg_a, FlagSet::default())
        .await
        .unwrap();
    store
        .set_auto_flags(&user(), conv(), msg_b, FlagSet::default())
        .await
        .unwrap();

    let records = MessageFlagsReader::list_for_conversation(&store, &user(), conv())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
}

#[tokio::test]
async fn list_for_conversation_does_not_include_a_different_conversations_messages() {
    let store = InMemoryMessageFlagsStore::new();
    let other_conv = ConversationId(uuid::Uuid::from_u128(999));
    store
        .set_auto_flags(&user(), conv(), msg(), FlagSet::default())
        .await
        .unwrap();
    store
        .set_auto_flags(&user(), other_conv, msg(), FlagSet::default())
        .await
        .unwrap();

    let records = MessageFlagsReader::list_for_conversation(&store, &user(), conv())
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
}

#[tokio::test]
async fn flags_are_isolated_per_user() {
    let store = InMemoryMessageFlagsStore::new();
    store
        .set_auto_flags(
            &user(),
            conv(),
            msg(),
            FlagSet {
                caps: true,
                critical: true,
                angry: true,
            },
        )
        .await
        .unwrap();

    let other_user = UserId("bob".to_string());
    assert!(MessageFlagsReader::get(&store, &other_user, conv(), msg())
        .await
        .unwrap()
        .is_none());
}
