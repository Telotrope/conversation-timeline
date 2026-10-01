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
