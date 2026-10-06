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
#[path = "support/upload_progress_contract.rs"]
mod upload_progress_contract;
#[macro_use]
#[path = "support/upload_received_contract.rs"]
mod upload_received_contract;
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

mod upload_progress {
    upload_progress_contract!(super::make);
}

mod upload_received {
    upload_received_contract!(super::make);
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
        name: ConversationName("only me".to_string()),
        version: 0,
        source: timeline_core::conversation_metadata::SourceFile {
            upload_id: upload(),
            file_name: timeline_core::labels::FileName::parse("conversations.json").unwrap(),
            uploaded_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            file_written_at: None,
        },
        additions: Vec::new(),
        message_count: 3,
        untimed: 0,
        message_span: None,
        participants: timeline_core::conversation_metadata::Participants::new(vec![
            timeline_core::conversation_metadata::Participant::Claude,
        ])
        .unwrap(),
        medium: timeline_core::conversation_metadata::ConversationMedium::Typed,
        details_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
        span: timeline_core::conversation_metadata::ConversationSpan::new(
            chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .fixed_offset(),
            chrono::DateTime::from_timestamp(1_700_003_600, 0)
                .unwrap()
                .fixed_offset(),
        )
        .unwrap(),
        span_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
        branch_of: None,
        branches: Vec::new(),
    };
    let stored = ConversationSummaryStore::put(&table, &alice(), summary.clone())
        .await
        .unwrap();
    assert_eq!(stored.version, 1);
    assert_eq!(table.list_for_user(&alice()).await.unwrap(), vec![stored]);
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
        name: ConversationName("x".to_string()),
        version: 0,
        source: timeline_core::conversation_metadata::SourceFile {
            upload_id: upload(),
            file_name: timeline_core::labels::FileName::parse("conversations.json").unwrap(),
            uploaded_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
            file_written_at: None,
        },
        additions: Vec::new(),
        message_count: 1,
        untimed: 0,
        message_span: None,
        participants: timeline_core::conversation_metadata::Participants::new(vec![
            timeline_core::conversation_metadata::Participant::Claude,
        ])
        .unwrap(),
        medium: timeline_core::conversation_metadata::ConversationMedium::Typed,
        details_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
        span: timeline_core::conversation_metadata::ConversationSpan::new(
            chrono::DateTime::from_timestamp(1_700_000_000, 0)
                .unwrap()
                .fixed_offset(),
            chrono::DateTime::from_timestamp(1_700_003_600, 0)
                .unwrap()
                .fixed_offset(),
        )
        .unwrap(),
        span_origin: timeline_core::conversation_metadata::MetadataOrigin::Guessed,
        branch_of: None,
        branches: Vec::new(),
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
    assert!(matches!(
        table.list_page(&alice(), None, 5).await,
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

/// A progress row whose fields have the wrong types, as only another program
/// could write it, is an error naming the field (plan
/// `2026-10-02-upload-processing-failures.md` §3).
#[tokio::test]
async fn a_progress_row_with_badly_typed_fields_is_an_error_naming_the_field() {
    let (store, raw, table) = make_with_raw_client().await;
    let user = UserId("alice".to_string());
    for (n, field, value) in [
        (1u128, "attempts", AttributeValue::S("two".to_string())),
        (2u128, "last_error", AttributeValue::N("5".to_string())),
    ] {
        let upload = UploadId(uuid::Uuid::from_u128(n));
        raw.put_item()
            .table_name(&table)
            .item("pk", AttributeValue::S("alice".to_string()))
            .item("sk", AttributeValue::S(format!("PROGRESS#{}", upload.0)))
            .item(field, value)
            .send()
            .await
            .unwrap();
        let err = store.get_progress(&user, upload).await.unwrap_err();
        assert!(err.to_string().contains(field), "{err}");
    }
}

#[tokio::test]
async fn progress_methods_report_a_missing_table_as_a_backend_error() {
    let client = dynamodb_local::client();
    let store = DynamoConversationsTable::new(client, "no-such-table");
    let (user, upload) = (
        UserId("alice".to_string()),
        UploadId(uuid::Uuid::from_u128(1)),
    );
    assert!(matches!(
        store.record_attempt(&user, upload).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store
            .record_attempt_error(&user, upload, "x".to_string())
            .await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        store.get_progress(&user, upload).await,
        Err(StoreError::Backend(_))
    ));
}

// ---- Conversation metadata (plan 2026-10-05-screen-flow.md §8a-8b) --------

/// A record with every new field set to something other than its guess,
/// so a field the adapter forgot to write or read would show.
fn full_record() -> ConversationSummary {
    use timeline_core::conversation_metadata::{
        ConversationMedium, ConversationSpan, MetadataOrigin, Participant, Participants,
        SourceFile, TranscriptionService,
    };
    use timeline_core::labels::{AiName, FileName, PersonName};
    let at = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap();
    ConversationSummary {
        conversation_id: conversation(),
        name: ConversationName("a meeting".to_string()),
        version: 0,
        source: SourceFile {
            upload_id: upload(),
            file_name: FileName::parse("meeting.json").unwrap(),
            uploaded_at: at("2026-10-05T12:00:00Z").with_timezone(&chrono::Utc),
            file_written_at: Some(at("2026-10-04T09:30:00Z").with_timezone(&chrono::Utc)),
        },
        additions: vec![
            UploadId(uuid::Uuid::from_u128(101)),
            UploadId(uuid::Uuid::from_u128(102)),
        ],
        message_count: 42,
        untimed: 5,
        message_span: Some(
            ConversationSpan::new(
                at("2026-10-01T10:00:00+00:00"),
                at("2026-10-01T11:00:00+00:00"),
            )
            .unwrap(),
        ),
        participants: Participants::new(vec![
            Participant::Human {
                name: PersonName::parse("Ada").unwrap(),
            },
            Participant::OtherAi {
                name: AiName::parse("Le Chat").unwrap(),
            },
        ])
        .unwrap(),
        medium: ConversationMedium::VirtualVoice {
            transcription: TranscriptionService::MicrosoftTeams,
        },
        details_origin: MetadataOrigin::Confirmed,
        span: ConversationSpan::new(
            at("2026-10-01T12:00:00+02:00"),
            at("2026-10-01T13:30:00+02:00"),
        )
        .unwrap(),
        span_origin: MetadataOrigin::Confirmed,
        branch_of: Some(ConversationId(uuid::Uuid::from_u128(201))),
        branches: vec![ConversationId(uuid::Uuid::from_u128(202))],
    }
}

#[tokio::test]
async fn every_metadata_field_survives_a_round_trip() {
    let (table, _keep) = make().await;
    let stored = ConversationSummaryStore::put(&table, &alice(), full_record())
        .await
        .unwrap();
    assert_eq!(
        stored,
        ConversationSummary {
            version: 1,
            ..full_record()
        }
    );
    assert_eq!(
        ConversationSummaryStore::get(&table, &alice(), conversation())
            .await
            .unwrap(),
        Some(stored.clone())
    );
    assert_eq!(table.list_for_user(&alice()).await.unwrap(), vec![stored]);
}

/// A row stored before conversations had metadata is refused, with a
/// message saying why, rather than shown with made-up file facts.
#[tokio::test]
async fn a_row_from_before_metadata_is_refused_with_a_message_saying_why() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &conversation_row(&[
            ("name", s("old chat")),
            ("message_count", AttributeValue::N("2".to_string())),
        ]),
    )
    .await;
    assert_backend_error_mentions(
        ConversationSummaryStore::get(&table, &alice(), conversation()).await,
        &["stored before conversations had metadata", "uploaded again"],
    );
}

/// A JSON attribute that doesn't parse says where it went wrong, never
/// what it held: these attributes hold names people typed.
#[tokio::test]
async fn an_unreadable_json_attribute_is_named_without_its_contents() {
    let (table, raw, name) = make_with_raw_client().await;
    put_raw(
        &raw,
        &name,
        &[
            ("pk", s("alice")),
            ("sk", s(&conversation_sk())),
            ("name", s("chat")),
            ("message_count", AttributeValue::N("2".to_string())),
            ("source", s(r#"{"secret": "Ada Lovelace""#)),
        ],
    )
    .await;
    let err = ConversationSummaryStore::get(&table, &alice(), conversation())
        .await
        .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("`source`") && text.contains("line 1"),
        "{text}"
    );
    assert!(!text.contains("Ada"), "{text}");
}

#[tokio::test]
async fn upload_facts_report_a_missing_table_as_a_backend_error() {
    let table = DynamoConversationsTable::new(dynamodb_local::client(), "no-such-table");
    let facts = timeline_core::conversation_metadata::UploadFacts {
        file_name: timeline_core::labels::FileName::parse("a.json").unwrap(),
        uploaded_at: chrono::Utc::now(),
        file_written_at: None,
        human_name: timeline_core::labels::PersonName::parse("Ada").unwrap(),
    };
    assert!(matches!(
        table.record_received(&alice(), upload(), facts).await,
        Err(StoreError::Backend(_))
    ));
    assert!(matches!(
        table.get_received(&alice(), upload()).await,
        Err(StoreError::Backend(_))
    ));
}
