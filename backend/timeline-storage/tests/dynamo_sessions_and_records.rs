//! `DynamoSessionStore` and `DynamoUserRecordStore` against Amazon's
//! DynamoDB Local: the shared session, user-record and saved-analysis
//! contracts, then rows our code didn't write and a missing table.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them -- see
//! `support/dynamodb_local.rs` for the setup steps.

#[macro_use]
#[path = "support/session_contract.rs"]
mod session_contract;
#[macro_use]
#[path = "support/user_record_contract.rs"]
mod user_record_contract;
#[path = "support/dynamodb_local.rs"]
mod dynamodb_local;

use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::flag_view::FlagView;
use timeline_core::model::ConversationId;
use timeline_core::ports::analyses::AnalysisStore;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::{Totals, UserRecordStore};
use timeline_core::server_analyses::{AnalysisRequest, SavedAnalysis, ServerAnalysis};
use timeline_storage::dynamo::sessions_table::DynamoSessionStore;
use timeline_storage::dynamo::user_record_rows::DynamoUserRecordStore;

async fn make_sessions() -> (DynamoSessionStore, ()) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (DynamoSessionStore::new(client, table), ())
}

async fn make_records() -> (DynamoUserRecordStore, ()) {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    (DynamoUserRecordStore::new(client, table), ())
}

mod sessions {
    session_contract!(super::make_sessions);
}

mod records {
    user_record_contract!(super::make_records);
}

fn alice() -> UserId {
    UserId("alice".to_string())
}

fn s(text: &str) -> AttributeValue {
    AttributeValue::S(text.to_string())
}

fn request() -> AnalysisRequest {
    AnalysisRequest {
        analysis: ServerAnalysis::TimeOfDay,
        view: FlagView::Both,
        zone: chrono_tz::UTC,
    }
}

#[tokio::test]
async fn a_session_row_whose_key_differs_from_its_session_is_a_backend_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoSessionStore::new(client.clone(), table.clone());
    let session = serde_json::json!({
        "conversation_id": ConversationId(uuid::Uuid::from_u128(1)),
        "number": 0,
        "start": "2026-01-01T00:00:00Z",
        "end": "2026-01-01T00:10:00Z",
        "placement": "gaps",
        "message_count": 2,
        "counts": {
            "messages": 1, "reviewed": 0,
            "automatic": {"caps": 0, "critical": 0, "angry": 0, "any": 0},
            "yours": {"caps": 0, "critical": 0, "angry": 0, "any": 0},
            "both": {"caps": 0, "critical": 0, "angry": 0, "any": 0}
        }
    });
    client
        .put_item()
        .table_name(&table)
        .item("pk", s("alice"))
        .item(
            "sk",
            s(&format!("SESS#{}#000007", uuid::Uuid::from_u128(1))),
        )
        .item("session", s(&session.to_string()))
        .send()
        .await
        .unwrap();
    match store.list_sessions(&alice()).await {
        Err(StoreError::Damaged(e)) => {
            assert!(
                e.to_string().contains("holds a session of a different key"),
                "{e}"
            )
        }
        other => panic!("expected a backend error, got {other:?}"),
    }
}

#[tokio::test]
async fn every_session_method_reports_a_missing_table_as_a_backend_error() {
    let store = DynamoSessionStore::new(dynamodb_local::client(), "no-such-table");
    let conversation = ConversationId(uuid::Uuid::from_u128(1));
    assert!(matches!(
        store.list_sessions(&alice()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.sessions_page(&alice(), None, 3).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.sessions_of(&alice(), conversation).await,
        Err(StoreError::Backend(_))
    ));
    let key = timeline_core::stored_session::SessionKey {
        conversation_id: conversation,
        number: 0,
    };
    assert!(matches!(
        store.delete_sessions(&alice(), &[key]).await,
        Err(StoreError::Backend(_))
    ));
}

#[tokio::test]
async fn every_record_and_analysis_method_reports_a_missing_table_as_a_backend_error() {
    let store = DynamoUserRecordStore::new(dynamodb_local::client(), "no-such-table");
    assert!(matches!(
        UserRecordStore::get(&store, &alice()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.raise_version(&alice()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.record_totals(&alice(), Totals::default()).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        AnalysisStore::get(&store, &alice(), &request().key()).await,
        Err(StoreError::Backend(_))
    ));
    let saved = SavedAnalysis {
        data_version: 1,
        numbers: request().empty(),
        unfinished: None,
    };
    assert!(matches!(
        AnalysisStore::put(&store, &alice(), &request().key(), &saved).await,
        Err(StoreError::Backend(_))
    ));
}

async fn record_with(attribute: &str, value: AttributeValue) -> String {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoUserRecordStore::new(client.clone(), table.clone());
    client
        .put_item()
        .table_name(&table)
        .item("pk", s("alice"))
        .item("sk", s("USER"))
        .item(attribute, value)
        .send()
        .await
        .unwrap();
    match UserRecordStore::get(&store, &alice()).await {
        Err(StoreError::Damaged(e)) => e.to_string(),
        other => panic!("expected a damaged-data error, got {other:?}"),
    }
}

#[tokio::test]
async fn a_record_number_of_the_wrong_type_is_an_error_naming_it() {
    let text = record_with("sessions", s("many")).await;
    assert!(text.contains("`sessions` should be a number"), "{text}");
}

#[tokio::test]
async fn a_record_number_that_is_not_whole_is_an_error_naming_it() {
    let text = record_with("messages", AttributeValue::N("1.5".to_string())).await;
    assert!(
        text.contains("`messages` should be a whole number of 0 or more"),
        "{text}"
    );
}

#[tokio::test]
async fn a_negative_data_version_is_an_error() {
    let text = record_with("data_version", AttributeValue::N("-1".to_string())).await;
    assert!(
        text.contains("`data_version` should be a whole number of 0 or more"),
        "{text}"
    );
}

#[tokio::test]
async fn a_saved_analysis_that_is_not_the_expected_json_is_an_error() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;
    let store = DynamoUserRecordStore::new(client.clone(), table.clone());
    client
        .put_item()
        .table_name(&table)
        .item("pk", s("alice"))
        .item("sk", s(&format!("ANALYSIS#{}", request().key())))
        .item("saved", s("{}"))
        .send()
        .await
        .unwrap();
    match AnalysisStore::get(&store, &alice(), &request().key()).await {
        Err(StoreError::Damaged(e)) => assert!(e.to_string().contains("`saved`"), "{e}"),
        other => panic!("expected a backend error, got {other:?}"),
    }
}
