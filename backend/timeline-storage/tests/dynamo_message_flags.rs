//! `DynamoMessageFlagsStore` against Amazon's DynamoDB Local. Runs the
//! shared message-flags contract -- which includes the public-API proof of
//! the auto/user separation (plan §4.1) -- then checks that need the real
//! table. See the migration plan's §V2b.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them -- see
//! `support/dynamodb_local.rs` for the setup steps.

#[macro_use]
#[path = "support/message_flags_contract.rs"]
mod message_flags_contract;
#[path = "support/dynamodb_local.rs"]
mod dynamodb_local;

use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::message_flags::{
    AutoFlagWriter, FlagOverrides, FlagSet, MessageFlagsReader, UserFlagWriter,
};
use timeline_storage::dynamo::message_flags_table::DynamoMessageFlagsStore;

async fn make() -> (DynamoMessageFlagsStore, ()) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (DynamoMessageFlagsStore::new(client, table), ())
}

mod contract {
    message_flags_contract!(super::make);
}

fn alice() -> UserId {
    UserId("alice".to_string())
}

fn conversation() -> ConversationId {
    ConversationId(uuid::Uuid::from_u128(1))
}

fn message() -> MessageId {
    MessageId(uuid::Uuid::from_u128(2))
}

/// The sort key is always a message id our own code wrote; a row whose
/// sort key isn't one is an inconsistency in our table and must surface as
/// an error, not be skipped or given a placeholder id.
#[tokio::test]
async fn a_row_whose_sort_key_is_not_a_message_id_is_a_backend_error_when_listed() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoMessageFlagsStore::new(client.clone(), table.clone());
    client
        .put_item()
        .table_name(&table)
        .item(
            "pk",
            AttributeValue::S(format!("{}#{}", alice(), conversation())),
        )
        .item("sk", AttributeValue::S("not-a-uuid".to_string()))
        .send()
        .await
        .expect("write raw test row");
    let got = store.list_for_conversation(&alice(), conversation()).await;
    assert!(matches!(got, Err(StoreError::Backend(_))), "got {got:?}");
}

#[tokio::test]
async fn every_method_reports_a_missing_table_as_a_backend_error() {
    let store = DynamoMessageFlagsStore::new(dynamodb_local::client(), "no-such-table");
    assert!(matches!(
        MessageFlagsReader::get(&store, &alice(), conversation(), message()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.list_for_conversation(&alice(), conversation()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store
            .set_auto_flags(&alice(), conversation(), message(), FlagSet::default())
            .await,
        Err(StoreError::Backend(_))
    ));
    let one_override = FlagOverrides {
        caps: Some(true),
        critical: None,
        angry: None,
    };
    assert!(matches!(
        store
            .set_user_flags(&alice(), conversation(), message(), one_override)
            .await,
        Err(StoreError::Backend(_))
    ));
}
