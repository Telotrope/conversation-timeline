//! Black-box tests for `process_upload` -- the orchestration function that
//! turns a raw upload into stored conversation summaries and auto flags.
//! Every assertion goes through the same port trait methods a real caller
//! (the local-dev endpoint, or a real S3-triggered Lambda) would use --
//! nothing reaches into the in-memory adapters' internals.
//!
//! Per the migration plan's §V2a-revision there is no pre-created upload
//! record to seed: a test harness just needs the raw bytes sitting at the
//! same key `process_upload` will recompute
//! ([`timeline_core::ports::uploads::raw_object_key`]), matching how a real
//! presigned-URL PUT lands the bytes before anything ever calls this
//! function.

#[path = "support/local_app.rs"]
mod local_app;

use std::sync::Arc;

use timeline_api::processing::{process_upload, ProcessingError};
use timeline_api::s3_trigger::ProcessingStores;
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::messages::AutoFlagWriter;
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome};
use timeline_core::stored_message::Entry;
use timeline_storage::memory::messages::InMemoryMessageStore;

const CONV_ID: &str = "11111111-1111-4111-8111-111111111111";
const HUMAN_MSG_ID: &str = "22222222-2222-4222-8222-222222222222";
const ASSISTANT_MSG_ID: &str = "33333333-3333-4333-8333-333333333333";
const HUMAN_TEXT: &str = "This is WRONG and you failed to fix it.";

fn sample_upload_json() -> String {
    format!(
        r#"[
  {{
    "uuid": "{CONV_ID}",
    "name": "Test Conversation",
    "chat_messages": [
      {{
        "uuid": "{HUMAN_MSG_ID}",
        "sender": "human",
        "created_at": "2024-01-01T00:00:00Z",
        "content": [{{"type": "text", "text": "{HUMAN_TEXT}"}}]
      }},
      {{
        "uuid": "{ASSISTANT_MSG_ID}",
        "sender": "assistant",
        "created_at": "2024-01-01T00:01:00Z",
        "content": [{{"type": "text", "text": "Let me try again."}}]
      }}
    ]
  }}
]"#
    )
}

/// The local server's processing stores, the message store also kept as
/// its concrete type so a test can write automatic flags as the scan does.
struct Harness {
    user_id: UserId,
    upload_id: UploadId,
    stores: ProcessingStores,
    messages: Arc<InMemoryMessageStore>,
}

impl Harness {
    async fn with_raw_bytes(raw: &[u8]) -> Self {
        let user_id = UserId("alice".to_string());
        let upload_id = UploadId(uuid::Uuid::from_u128(1));
        let messages = Arc::new(InMemoryMessageStore::new());
        let mut stores = local_app::memory_stores();
        stores.message_reader = messages.clone();
        stores.message_writer = messages.clone();
        let key = raw_object_key(&user_id, upload_id);
        stores.object_store.put(&key, raw.to_vec()).await.unwrap();
        local_app::record_upload_facts(&stores, &user_id, upload_id).await;
        Harness {
            user_id,
            upload_id,
            stores,
            messages,
        }
    }

    async fn run(&self) -> Result<(), ProcessingError> {
        process_upload(&self.stores, &self.user_id, self.upload_id).await
    }

    /// The flags on `message`'s row, `None` for a message with no row or
    /// one that carries none (Claude's).
    async fn flags(&self, message: MessageId) -> Option<MessageFlags> {
        match self
            .stores
            .message_reader
            .find_entry(
                &self.user_id,
                ConversationId(CONV_ID.parse().unwrap()),
                message,
            )
            .await
            .unwrap()
        {
            Some(Entry::Message(m)) => m.flags,
            _ => None,
        }
    }

    async fn has_row(&self, message: MessageId) -> bool {
        self.stores
            .message_reader
            .find_entry(
                &self.user_id,
                ConversationId(CONV_ID.parse().unwrap()),
                message,
            )
            .await
            .unwrap()
            .is_some()
    }
}

#[tokio::test]
async fn a_successful_upload_produces_a_summary_and_records_a_ready_outcome() {
    let h = Harness::with_raw_bytes(sample_upload_json().as_bytes()).await;
    h.run().await.unwrap();

    let conversation_id = ConversationId(CONV_ID.parse().unwrap());
    let outcome = h
        .stores
        .upload_outcome_store
        .get_outcome(&h.user_id, h.upload_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        outcome,
        UploadOutcome::Ready {
            conversation_ids: vec![conversation_id]
        }
    );

    let summaries = h
        .stores
        .conversation_summary_store
        .list_for_user(&h.user_id)
        .await
        .unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].conversation_id, conversation_id);
    assert_eq!(summaries[0].name.0, "Test Conversation");
    assert_eq!(summaries[0].message_count, 2);
}

#[tokio::test]
async fn processing_an_upload_writes_no_automatic_flags() {
    // Detection is a separate, user-triggered pass now (POST /detect); the
    // assertions about what it computes live in tests/detect.rs. What upload
    // processing must guarantee is the negative: it does not run a pass over
    // every speech act just because a file arrived.
    let h = Harness::with_raw_bytes(sample_upload_json().as_bytes()).await;
    h.run().await.unwrap();

    // Flags now live on the message rows (plan
    // 2026-10-06-load-only-what-the-page-shows.md §10b): every message has a
    // row, and none has automatic flags.
    for message_id in [HUMAN_MSG_ID, ASSISTANT_MSG_ID] {
        let id = MessageId(message_id.parse().unwrap());
        assert!(h.has_row(id).await, "{message_id} has a row");
        assert_eq!(
            h.flags(id).await.and_then(|f| f.auto),
            None,
            "upload processing must not write auto flags for {message_id}"
        );
    }
}

#[tokio::test]
async fn invalid_json_records_a_failed_outcome_with_a_reason() {
    let h = Harness::with_raw_bytes(b"not json at all").await;
    let err = h.run().await.unwrap_err();
    assert!(matches!(err, ProcessingError::Format(_)));

    let outcome = h
        .stores
        .upload_outcome_store
        .get_outcome(&h.user_id, h.upload_id)
        .await
        .unwrap()
        .unwrap();
    match outcome {
        UploadOutcome::Failed { reason } => assert!(!reason.is_empty()),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn processing_an_upload_whose_bytes_were_never_put_is_an_object_store_not_found() {
    let user_id = UserId("alice".to_string());
    let upload_id = UploadId(uuid::Uuid::from_u128(999));
    let stores = local_app::memory_stores();

    let err = process_upload(&stores, &user_id, upload_id)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        ProcessingError::ObjectStore(ObjectStoreError::NotFound)
    ));
}

/// The sample upload with a `_claude_timeline_user` field on each message.
fn upload_with_reviews(human_review: &str, assistant_review: &str) -> String {
    sample_upload_json()
        .replacen(
            r#""content": [{"type": "text", "text": "This"#,
            &format!(r#""_claude_timeline_user": {human_review}, "content": [{{"type": "text", "text": "This"#),
            1,
        )
        .replacen(
            r#""content": [{"type": "text", "text": "Let me"#,
            &format!(r#""_claude_timeline_user": {assistant_review}, "content": [{{"type": "text", "text": "Let me"#),
            1,
        )
}

fn ids() -> (ConversationId, MessageId, MessageId) {
    (
        ConversationId(CONV_ID.parse().unwrap()),
        MessageId(HUMAN_MSG_ID.parse().unwrap()),
        MessageId(ASSISTANT_MSG_ID.parse().unwrap()),
    )
}

#[tokio::test]
async fn reviews_embedded_in_an_upload_are_stored_as_yours() {
    let raw = upload_with_reviews(
        r#"{"caps": false, "critical": true, "angry": false}"#,
        r#"{"caps": true, "critical": true, "angry": true}"#,
    );
    let h = Harness::with_raw_bytes(raw.as_bytes()).await;
    h.run().await.unwrap();

    let (_, human, assistant) = ids();
    let record = h.flags(human).await.unwrap();
    assert_eq!(
        record.user,
        FlagOverrides {
            caps: Some(false),
            critical: Some(true),
            angry: Some(false)
        }
    );
    // Only your own messages carry reviews; one on Claude's is ignored.
    assert!(h.has_row(assistant).await);
    assert_eq!(h.flags(assistant).await, None);
}

#[tokio::test]
async fn an_empty_review_is_not_stored() {
    let raw = upload_with_reviews(r#"{"caps": null, "critical": null, "angry": null}"#, "{}");
    let h = Harness::with_raw_bytes(raw.as_bytes()).await;
    h.run().await.unwrap();

    let (_, human, _) = ids();
    // The row exists (every message has one), with no review stored.
    assert_eq!(h.flags(human).await, Some(MessageFlags::default()));
}

#[tokio::test]
async fn detection_after_upload_keeps_the_imported_review() {
    // The failure this import exists to prevent: detection used to create
    // records whose empty review then replaced the file's in the export.
    let raw = upload_with_reviews(r#"{"caps": false, "critical": true, "angry": false}"#, "{}");
    let h = Harness::with_raw_bytes(raw.as_bytes()).await;
    h.run().await.unwrap();

    let (conv, human, _) = ids();
    let key = timeline_core::stored_message::EntryKey {
        conversation_id: conv,
        at: chrono::DateTime::parse_from_rfc3339("2024-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc),
        id: human,
    };
    h.messages
        .set_auto_flags(
            &h.user_id,
            key,
            FlagSet {
                caps: true,
                critical: false,
                angry: true,
            },
        )
        .await
        .unwrap();
    let record = h.flags(human).await.unwrap();
    assert_eq!(record.user.critical, Some(true));
    assert_eq!(
        record.auto,
        Some(FlagSet {
            caps: true,
            critical: false,
            angry: true
        })
    );
}

#[tokio::test]
async fn an_unreadable_review_fails_the_upload_and_stores_nothing() {
    let raw = upload_with_reviews(r#""yes please""#, "{}");
    let h = Harness::with_raw_bytes(raw.as_bytes()).await;
    let err = h.run().await.unwrap_err();
    assert!(
        matches!(err, ProcessingError::ReviewField { .. }),
        "{err:?}"
    );

    let outcome = h
        .stores
        .upload_outcome_store
        .get_outcome(&h.user_id, h.upload_id)
        .await
        .unwrap()
        .unwrap();
    let UploadOutcome::Failed { reason } = outcome else {
        panic!("expected Failed, got {outcome:?}")
    };
    assert!(
        reason.contains(HUMAN_MSG_ID) && reason.contains("_claude_timeline_user"),
        "{reason}"
    );

    let (_, human, _) = ids();
    assert!(!h.has_row(human).await);
    assert!(h
        .stores
        .conversation_summary_store
        .list_for_user(&h.user_id)
        .await
        .unwrap()
        .is_empty());
}
