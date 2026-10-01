//! `DynamoConversationsTable` against Amazon's DynamoDB Local. Runs the
//! shared `UploadOutcomeStore` and `ConversationSummaryStore` contracts,
//! then checks that need the real table: both kinds of row sharing one
//! table, rows our own code didn't write, and a missing table. See the
//! migration plan's §V2b.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them -- see
//! `support/dynamodb_local.rs` for the setup steps.

#[macro_use]
#[path = "support/upload_outcome_contract.rs"]
mod upload_outcome_contract;
#[macro_use]
#[path = "support/conversation_summary_contract.rs"]
mod conversation_summary_contract;
#[path = "support/dynamodb_local.rs"]
mod dynamodb_local;

use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;

async fn make() -> (DynamoConversationsTable, ()) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (DynamoConversationsTable::new(client, table), ())
}

/// A table plus a raw client, for writing rows the adapter didn't write.
async fn make_with_raw_client() -> (DynamoConversationsTable, aws_sdk_dynamodb::Client, String) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (
        DynamoConversationsTable::new(client.clone(), table.clone()),
        client,
        table,
    )
}

mod upload_outcomes {
    upload_outcome_contract!(super::make);
}

mod conversation_summaries {
    conversation_summary_contract!(super::make);
}

fn alice() -> UserId {
    UserId("alice".to_string())
}

fn upload() -> UploadId {
    UploadId(uuid::Uuid::from_u128(100))
}

fn conversation() -> ConversationId {
    ConversationId(uuid::Uuid::from_u128(1))
}

async fn put_raw(client: &aws_sdk_dynamodb::Client, table: &str, attrs: &[(&str, AttributeValue)]) {
    let mut req = client.put_item().table_name(table);
    for (k, v) in attrs {
        req = req.item(*k, v.clone());
    }
    req.send().await.expect("write raw test row");
}

fn s(v: &str) -> AttributeValue {
    AttributeValue::S(v.to_string())
}

/// Upload-outcome rows and conversation-summary rows share one table,
/// told apart by sort-key prefix; listing summaries must never return an
/// upload's row.
#[tokio::test]
async fn list_for_user_skips_upload_outcome_rows_in_the_same_table() {
    let (table, _keep) = make().await;
    table
        .record_outcome(
            &alice(),
            upload(),
            UploadOutcome::Ready {
                conversation_ids: vec![conversation()],
            },
        )
        .await
        .unwrap();
    let summary = ConversationSummary {
        conversation_id: conversation(),
        upload_id: upload(),
        name: ConversationName("only me".to_string()),
        message_count: 3,
    };
    ConversationSummaryStore::put(&table, &alice(), summary.clone())
        .await
        .unwrap();
    assert_eq!(table.list_for_user(&alice()).await.unwrap(), vec![summary]);
}

#[tokio::test]
async fn an_upload_row_without_a_status_is_a_backend_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&format!("UPLOAD#{}", upload()))),
        ],
    )
    .await;
    let got = table.get_outcome(&alice(), upload()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

#[tokio::test]
async fn an_upload_row_with_an_unknown_status_is_a_backend_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&format!("UPLOAD#{}", upload()))),
            ("status", s("pending")),
        ],
    )
    .await;
    let got = table.get_outcome(&alice(), upload()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

#[tokio::test]
async fn a_conversation_row_without_an_upload_id_is_a_backend_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&format!("CONV#{}", conversation()))),
        ],
    )
    .await;
    let got = ConversationSummaryStore::get(&table, &alice(), conversation()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

#[tokio::test]
async fn a_conversation_row_with_a_malformed_upload_id_is_a_backend_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&format!("CONV#{}", conversation()))),
            ("upload_id", s("not-a-uuid")),
        ],
    )
    .await;
    let got = ConversationSummaryStore::get(&table, &alice(), conversation()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

#[tokio::test]
async fn a_conversation_row_whose_sort_key_is_not_a_uuid_is_a_backend_error_when_listed() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s("CONV#not-a-uuid")),
            ("upload_id", s(&upload().to_string())),
        ],
    )
    .await;
    let got = table.list_for_user(&alice()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

/// Every method reports a missing table as `Backend` -- a misconfiguration,
/// never `NotFound` or an empty result.
#[tokio::test]
async fn every_method_reports_a_missing_table_as_a_backend_error() {
    let table = DynamoConversationsTable::new(dynamodb_local::client(), "no-such-table");
    let summary = ConversationSummary {
        conversation_id: conversation(),
        upload_id: upload(),
        name: ConversationName("x".to_string()),
        message_count: 1,
    };
    let failed = UploadOutcome::Failed {
        reason: "x".to_string(),
    };
    assert!(matches!(
        table.record_outcome(&alice(), upload(), failed).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        table.get_outcome(&alice(), upload()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        table.list_for_user(&alice()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        ConversationSummaryStore::put(&table, &alice(), summary).await,
        Err(StoreError::Backend(_))
    ));
}

// ---- Malformed values: errors, never silent defaults (migration plan §V2c).
// Each test writes a row the adapter would never write, then reads it back
// through the real trait method. The error must say which attribute is
// wrong and what was found.

fn assert_backend_error_mentions<T: std::fmt::Debug>(
    result: Result<T, StoreError>,
    expected: &[&str],
) {
    match result {
        Err(StoreError::Backend(e)) => {
            let text = e.to_string();
            for piece in expected {
                assert!(
                    text.contains(piece),
                    "error {text:?} should mention {piece:?}"
                );
            }
        }
        other => panic!("expected a Backend error mentioning {expected:?}, got {other:?}"),
    }
}

fn upload_sk() -> String {
    format!("UPLOAD#{}", upload())
}

fn conversation_sk() -> String {
    format!("CONV#{}", conversation())
}

#[tokio::test]
async fn a_ready_upload_without_conversation_ids_is_an_error_not_an_empty_list() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("ready")),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`conversation_ids`", "missing", &upload_sk()],
    );
}

#[tokio::test]
async fn conversation_ids_stored_as_a_map_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("ready")),
            ("conversation_ids", AttributeValue::M(Default::default())),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`conversation_ids`", "should be a list", "is a map"],
    );
}

#[tokio::test]
async fn a_conversation_id_entry_that_is_not_a_string_is_an_error_not_dropped() {
    let (table, raw, name) = make_with_raw_client().await;
    let ids = AttributeValue::L(vec![
        s(&conversation().to_string()),
        AttributeValue::N("7".to_string()),
    ]);
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("ready")),
            ("conversation_ids", ids),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`conversation_ids[1]`", "should be a string", "is a number"],
    );
}

#[tokio::test]
async fn a_conversation_id_entry_that_is_not_a_valid_id_is_an_error_not_dropped() {
    let (table, raw, name) = make_with_raw_client().await;
    let long_bad_id = format!("not-an-id\n{}", "x".repeat(100));
    let ids = AttributeValue::L(vec![s(&conversation().to_string()), s(&long_bad_id)]);
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("ready")),
            ("conversation_ids", ids),
        ],
    )
    .await;
    let result = table.get_outcome(&alice(), upload()).await;
    // The bad value is shown escaped (no raw newline) and cut short.
    let text = format!("{result:?}");
    assert!(
        !text.contains(&"x".repeat(100)),
        "bad value must be cut short: {text}"
    );
    assert_backend_error_mentions(
        result,
        &[
            "`conversation_ids[1]`",
            "should be an id",
            "not-an-id\\\\n",
            "…",
        ],
    );
}

#[tokio::test]
async fn a_failed_upload_without_a_failure_reason_is_an_error_not_an_empty_string() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("failed")),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`failure_reason`", "missing"],
    );
}

#[tokio::test]
async fn a_failure_reason_stored_as_a_number_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", s("failed")),
            ("failure_reason", AttributeValue::N("3".to_string())),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`failure_reason`", "should be a string", "is a number"],
    );
}

#[tokio::test]
async fn a_status_stored_as_a_list_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&upload_sk())),
            ("status", AttributeValue::L(vec![])),
        ],
    )
    .await;
    assert_backend_error_mentions(
        table.get_outcome(&alice(), upload()).await,
        &["`status`", "should be a string", "is a list"],
    );
}

fn conversation_row(
    extra: &[(&'static str, AttributeValue)],
) -> Vec<(&'static str, AttributeValue)> {
    let mut attrs = vec![
        ("pk", s("alice")),
        ("sk", s(&conversation_sk())),
        ("upload_id", s(&upload().to_string())),
    ];
    attrs.extend(extra.iter().cloned());
    attrs
}

#[tokio::test]
async fn a_conversation_without_a_name_is_an_error_not_an_empty_string() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[("message_count", AttributeValue::N("2".to_string()))]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`name`", "missing", &conversation_sk()],
    );
}

#[tokio::test]
async fn a_name_stored_as_null_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[
            ("name", AttributeValue::Null(true)),
            ("message_count", AttributeValue::N("2".to_string())),
        ]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`name`", "should be a string", "is null"],
    );
}

#[tokio::test]
async fn a_conversation_without_a_message_count_is_an_error_not_zero() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(&raw, &name, &conversation_row(&[("name", s("a"))])).await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`message_count`", "missing"],
    );
}

#[tokio::test]
async fn a_message_count_stored_as_a_string_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[("name", s("a")), ("message_count", s("2"))]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`message_count`", "should be a number", "is a string"],
    );
}

#[tokio::test]
async fn a_negative_message_count_is_an_error_not_zero() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[
            ("name", s("a")),
            ("message_count", AttributeValue::N("-1".to_string())),
        ]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`message_count`", "whole number", "-1"],
    );
}

/// Listing reads summaries the same way `get` does, so one malformed row
/// fails the list instead of being skipped or shown with defaults.
#[tokio::test]
async fn listing_fails_on_a_malformed_summary_instead_of_skipping_it() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(&raw, &name, &conversation_row(&[("name", s("a"))])).await;
    assert_backend_error_mentions(
        table.list_for_user(&alice()).await,
        &["`message_count`", "missing"],
    );
}

#[tokio::test]
async fn a_name_stored_as_true_or_false_is_an_error() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[
            ("name", AttributeValue::Bool(true)),
            ("message_count", AttributeValue::N("2".to_string())),
        ]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["`name`", "should be a string", "is a true/false value"],
    );
}
