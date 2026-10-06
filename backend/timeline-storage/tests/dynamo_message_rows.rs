//! `DynamoMessageStore` against Amazon's DynamoDB Local. Runs the shared
//! message-row contract -- which includes the public-API proof of the
//! automatic/yours separation (migration plan §4.1) -- then checks that need
//! the real table: rows our code didn't write, a missing table, reading past
//! DynamoDB's 1 MB per answer, and (against a stand-in, since DynamoDB Local
//! never does it) batches DynamoDB hands back unfinished.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them -- see
//! `support/dynamodb_local.rs` for the setup steps.

#[macro_use]
#[path = "support/message_rows_contract.rs"]
mod message_rows_contract;
#[path = "support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "support/fake_dynamodb.rs"]
mod fake_dynamodb;

use std::sync::atomic::Ordering;

use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{
    AutoFlagWriter, EntryRange, MessageReader, MessageRowWriter, UserFlagWriter,
};
use timeline_core::stored_message::{Entry, EntryKey, Piece, StoredMessage};
use timeline_storage::dynamo::message_rows::DynamoMessageStore;

async fn make() -> (DynamoMessageStore, ()) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (DynamoMessageStore::new(client, table), ())
}

mod contract {
    message_rows_contract!(super::make);
}

fn alice() -> UserId {
    UserId("alice".to_string())
}

fn conversation() -> ConversationId {
    ConversationId(uuid::Uuid::from_u128(1))
}

fn key(minute: i64, id: u128) -> EntryKey {
    EntryKey {
        conversation_id: conversation(),
        at: chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0).unwrap(),
        id: MessageId(uuid::Uuid::from_u128(id)),
    }
}

fn yours(key: EntryKey, text: String) -> Entry {
    Entry::Message(StoredMessage {
        key,
        parent: None,
        sender: Sender::Human,
        pieces: vec![Piece::Text {
            text,
            citations: Vec::new(),
        }],
        attachments: Vec::new(),
        flags: Some(MessageFlags::default()),
    })
}

fn whole() -> EntryRange {
    EntryRange {
        conversation_id: conversation(),
        times: None,
        after: None,
    }
}

/// The sort key [`DynamoMessageStore`] writes for `key`.
fn sk(key: EntryKey) -> String {
    format!(
        "MSG#{}#{}#{}",
        key.conversation_id,
        key.at.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
        key.id
    )
}

async fn put_raw(
    client: &aws_sdk_dynamodb::Client,
    table: &str,
    item: Vec<(&str, AttributeValue)>,
) {
    let mut request = client.put_item().table_name(table);
    for (name, value) in item {
        request = request.item(name, value);
    }
    request.send().await.expect("write raw test row");
}

fn s(text: &str) -> AttributeValue {
    AttributeValue::S(text.to_string())
}

/// A row as the adapter writes it for your unflagged message under `k`.
fn row(k: EntryKey) -> Vec<(&'static str, AttributeValue)> {
    let mut entry = yours(k, "hello".to_string());
    if let Entry::Message(m) = &mut entry {
        m.flags = None;
    }
    vec![
        ("pk", s("alice")),
        ("sk", s(&sk(k))),
        ("message_id", s(&k.id.to_string())),
        ("yours", AttributeValue::Bool(true)),
        ("entry", s(&serde_json::to_string(&entry).unwrap())),
    ]
}

/// Replaces `dynamo_message_flags.rs`'s
/// `a_row_whose_sort_key_is_not_a_message_id_is_a_backend_error_when_listed`:
/// a message row whose key isn't a conversation, time and id is an error,
/// never skipped or given a made-up key.
#[tokio::test]
async fn a_message_row_whose_key_is_not_a_conversation_time_and_id_is_a_backend_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    for bad in [
        format!(
            "MSG#{}#not-a-time#{}",
            conversation(),
            uuid::Uuid::from_u128(2)
        ),
        format!("MSG#{}#2026-01-01T00:00:00Z#not-an-id", conversation()),
        format!("MSG#{}#2026-01-01T00:00:00Z", conversation()),
    ] {
        put_raw(&client, &table, vec![("pk", s("alice")), ("sk", s(&bad))]).await;
        let got = store.read_entries(&alice(), whole()).await;
        match got {
            Err(StoreError::Backend(e)) => assert!(
                e.to_string().contains("is not a conversation, time and id"),
                "{e}"
            ),
            other => panic!("expected a backend error for {bad:?}, got {other:?}"),
        }
        client
            .delete_item()
            .table_name(&table)
            .key("pk", s("alice"))
            .key("sk", s(&bad))
            .send()
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_row_whose_entry_has_another_key_is_a_backend_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    let mut item = row(key(0, 2));
    item[1] = ("sk", s(&sk(key(5, 2))));
    put_raw(&client, &table, item).await;
    match store.read_entries(&alice(), whole()).await {
        Err(StoreError::Backend(e)) => {
            assert!(
                e.to_string()
                    .contains("holds an entry with a different key"),
                "{e}"
            )
        }
        other => panic!("expected a backend error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_row_whose_entry_is_not_the_expected_json_is_a_backend_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    let mut item = row(key(0, 2));
    item[4] = ("entry", s(r#"{"entry": "message""#));
    put_raw(&client, &table, item).await;
    match store.read_entries(&alice(), whole()).await {
        Err(StoreError::Backend(e)) => assert!(e.to_string().contains("`entry`"), "{e}"),
        other => panic!("expected a backend error, got {other:?}"),
    }
}

/// Replaces `dynamo_message_flags.rs`'s
/// `every_method_reports_a_missing_table_as_a_backend_error`.
#[tokio::test]
async fn every_message_store_method_reports_a_missing_table_as_a_backend_error() {
    let store = DynamoMessageStore::new(dynamodb_local::client(), "no-such-table");
    let k = key(0, 2);
    assert!(matches!(
        store.read_entries(&alice(), whole()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.find_entry(&alice(), conversation(), k.id).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.entry_after(&alice(), k).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store
            .put_entries(&alice(), &[yours(k, "x".to_string())])
            .await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.delete_entries(&alice(), &[k]).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.set_auto_flags(&alice(), k, FlagSet::default()).await,
        Err(StoreError::Backend(_))
    ));
    let one = FlagOverrides {
        caps: Some(true),
        critical: None,
        angry: None,
    };
    assert!(matches!(
        store.set_user_flags(&alice(), k, one).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store
            .set_user_flags(&alice(), k, FlagOverrides::default())
            .await,
        Err(StoreError::Backend(_))
    ));
}

// ---- Flag values of the wrong type: errors, never silently "not flagged"
// (migration plan §V2c). Replace `dynamo_message_flags.rs`'s six
// `…_stored_as_…_is_an_error` tests.

async fn read_with_one_bad_attribute(attribute: &'static str, value: AttributeValue) -> String {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    let mut item = row(key(0, 2));
    item.push((attribute, value));
    put_raw(&client, &table, item).await;
    let found = store
        .find_entry(&alice(), conversation(), key(0, 2).id)
        .await;
    assert!(
        matches!(found, Err(StoreError::Backend(_))),
        "finding must fail too: {found:?}"
    );
    match store.read_entries(&alice(), whole()).await {
        Err(StoreError::Backend(e)) => e.to_string(),
        other => panic!("expected a Backend error for {attribute}, got {other:?}"),
    }
}

fn assert_mentions(text: &str, pieces: &[&str]) {
    for piece in pieces {
        assert!(
            text.contains(piece),
            "error {text:?} should mention {piece:?}"
        );
    }
}

#[tokio::test]
async fn auto_caps_stored_as_a_string_is_an_error() {
    let text = read_with_one_bad_attribute("auto_caps", s("true")).await;
    assert_mentions(
        &text,
        &["`auto_caps`", "should be a true/false value", "is a string"],
    );
}

#[tokio::test]
async fn auto_critical_stored_as_a_number_is_an_error() {
    let text =
        read_with_one_bad_attribute("auto_critical", AttributeValue::N("1".to_string())).await;
    assert_mentions(&text, &["`auto_critical`", "is a number"]);
}

#[tokio::test]
async fn auto_angry_stored_as_a_string_set_is_an_error() {
    let text =
        read_with_one_bad_attribute("auto_angry", AttributeValue::Ss(vec!["yes".to_string()]))
            .await;
    assert_mentions(&text, &["`auto_angry`", "is an unsupported type"]);
}

#[tokio::test]
async fn user_caps_stored_as_null_is_an_error() {
    let text = read_with_one_bad_attribute("user_caps", AttributeValue::Null(true)).await;
    assert_mentions(&text, &["`user_caps`", "is null"]);
}

#[tokio::test]
async fn user_critical_stored_as_a_list_is_an_error() {
    let text = read_with_one_bad_attribute("user_critical", AttributeValue::L(vec![])).await;
    assert_mentions(&text, &["`user_critical`", "is a list"]);
}

#[tokio::test]
async fn user_angry_stored_as_a_map_is_an_error() {
    let text =
        read_with_one_bad_attribute("user_angry", AttributeValue::M(Default::default())).await;
    assert_mentions(&text, &["`user_angry`", "is a map"]);
}

#[tokio::test]
async fn a_yours_marker_of_the_wrong_type_is_an_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    let mut item = row(key(0, 2));
    item[3] = ("yours", s("yes"));
    put_raw(&client, &table, item).await;
    match store.read_entries(&alice(), whole()).await {
        Err(StoreError::Backend(e)) => assert_mentions(&e.to_string(), &["`yours`", "is a string"]),
        other => panic!("expected a backend error, got {other:?}"),
    }
    let empty = store
        .set_user_flags(&alice(), key(0, 2), FlagOverrides::default())
        .await;
    assert!(
        matches!(empty, Err(StoreError::Backend(_))),
        "got {empty:?}"
    );
}

/// An automatic flag left out by something other than the scan (which
/// always writes all three) reads as not flagged, as before.
#[tokio::test]
async fn an_automatic_flag_written_alone_reads_the_others_as_not_flagged() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageStore::new(client.clone(), table.clone());
    let mut item = row(key(0, 2));
    item.push(("auto_critical", AttributeValue::Bool(true)));
    put_raw(&client, &table, item).await;
    let got = store.read_entries(&alice(), whole()).await.unwrap();
    let Entry::Message(m) = &got[0] else {
        panic!("expected a message, got {got:?}");
    };
    assert_eq!(
        m.flags.unwrap().auto,
        Some(FlagSet {
            caps: false,
            critical: true,
            angry: false
        })
    );
}

/// DynamoDB answers at most 1 MB at a time; a conversation larger than that
/// is still read whole (the cut-off the screen-flow analysis found in
/// `list_for_user`).
#[tokio::test]
async fn a_conversation_larger_than_one_answer_is_read_whole() {
    let (store, _keep) = make().await;
    let entries: Vec<Entry> = (0..30)
        .map(|i| yours(key(i, 100 + i as u128), "x".repeat(60_000)))
        .collect();
    store.put_entries(&alice(), &entries).await.unwrap();
    let got = store.read_entries(&alice(), whole()).await.unwrap();
    assert_eq!(got.len(), 30);
    assert_eq!(got, entries);
}

/// Rows DynamoDB hands back unfinished are sent again, and a write that
/// finishes on a later try succeeds.
#[tokio::test]
async fn rows_handed_back_unfinished_are_sent_again() {
    let fake = fake_dynamodb::start(fake_dynamodb::Busy::FirstAnswers(2)).await;
    let store = DynamoMessageStore::new(fake.client.clone(), "busy-table");
    let entries = vec![yours(key(0, 2), "a".to_string())];
    store.put_entries(&alice(), &entries).await.unwrap();
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
}

/// After every try, the rows still unwritten are reported by count, and the
/// write fails (plan §7).
#[tokio::test]
async fn rows_still_unfinished_after_every_try_are_reported_by_count() {
    let fake = fake_dynamodb::start(fake_dynamodb::Busy::Always).await;
    let store = DynamoMessageStore::new(fake.client.clone(), "busy-table");
    let entries: Vec<Entry> = (0..30)
        .map(|i| yours(key(i, 100 + i as u128), "a".to_string()))
        .collect();
    let got = store.put_entries(&alice(), &entries).await;
    assert!(
        matches!(
            got,
            Err(StoreError::Unwritten {
                left: 30,
                total: 30
            })
        ),
        "got {got:?}"
    );
    // Two batches (25 and 5), five tries each.
    assert_eq!(fake.calls.load(Ordering::SeqCst), 10);
    assert_eq!(
        got.unwrap_err().to_string(),
        "30 of 30 rows were still unwritten after every retry"
    );
}
