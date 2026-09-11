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

use timeline_core::flags::anger::detect_angry;
use timeline_core::flags::caps::has_emphasis_caps;
use timeline_core::flags::criticism::detect_critical;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::errors::ObjectStoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::message_flags::{FlagSet, MessageFlagsReader};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome, UploadOutcomeStore};
use timeline_api::processing::{process_upload, ProcessingError};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

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

struct Harness {
    user_id: UserId,
    upload_id: UploadId,
    object_store: InMemoryObjectStore,
    upload_outcome_store: InMemoryUploadOutcomeStore,
    conversation_summary_store: InMemoryConversationSummaryStore,
    flags_store: InMemoryMessageFlagsStore,
}

impl Harness {
    async fn with_raw_bytes(raw: &[u8]) -> Self {
        let user_id = UserId("alice".to_string());
        let upload_id = UploadId(uuid::Uuid::from_u128(1));
        let object_store = InMemoryObjectStore::new();
        let key = raw_object_key(&user_id, upload_id);
        object_store.put(&key, raw.to_vec()).await.unwrap();
        Harness {
            user_id,
            upload_id,
            object_store,
            upload_outcome_store: InMemoryUploadOutcomeStore::new(),
            conversation_summary_store: InMemoryConversationSummaryStore::new(),
            flags_store: InMemoryMessageFlagsStore::new(),
        }
    }

    async fn run(&self) -> Result<(), ProcessingError> {
        process_upload(
            &self.object_store,
            &self.upload_outcome_store,
            &self.conversation_summary_store,
            &self.flags_store,
            &self.user_id,
            self.upload_id,
        )
        .await
    }
}

#[tokio::test]
async fn a_successful_upload_produces_a_summary_and_records_a_ready_outcome() {
    let h = Harness::with_raw_bytes(sample_upload_json().as_bytes()).await;
    h.run().await.unwrap();

    let conversation_id = ConversationId(CONV_ID.parse().unwrap());
    let outcome = h
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
async fn human_message_gets_the_real_heuristic_flags_not_a_hardcoded_stand_in() {
    let h = Harness::with_raw_bytes(sample_upload_json().as_bytes()).await;
    h.run().await.unwrap();

    let conversation_id = ConversationId(CONV_ID.parse().unwrap());
    let human_id = MessageId(HUMAN_MSG_ID.parse().unwrap());
    let record = h
        .flags_store
        .get(&h.user_id, conversation_id, human_id)
        .await
        .unwrap()
        .expect("human message should have an auto-flag record");

    let expected = FlagSet {
        caps: has_emphasis_caps(HUMAN_TEXT),
        critical: detect_critical(HUMAN_TEXT),
        angry: detect_angry(HUMAN_TEXT),
    };
    assert_eq!(record.auto, expected);
    // The example text is specifically chosen to exercise the caps and
    // criticism heuristics from timeline-project-decisions.md section 5.
    assert!(expected.caps, "WRONG should trip the caps heuristic");
    assert!(
        expected.critical,
        "'you failed to' should trip the criticism heuristic"
    );
}

#[tokio::test]
async fn assistant_messages_never_get_an_auto_flag_record() {
    let h = Harness::with_raw_bytes(sample_upload_json().as_bytes()).await;
    h.run().await.unwrap();

    let conversation_id = ConversationId(CONV_ID.parse().unwrap());
    let assistant_id = MessageId(ASSISTANT_MSG_ID.parse().unwrap());
    let record = h
        .flags_store
        .get(&h.user_id, conversation_id, assistant_id)
        .await
        .unwrap();
    assert!(record.is_none());
}

#[tokio::test]
async fn invalid_json_records_a_failed_outcome_with_a_reason() {
    let h = Harness::with_raw_bytes(b"not json at all").await;
    let err = h.run().await.unwrap_err();
    assert!(matches!(err, ProcessingError::Format(_)));

    let outcome = h
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
    let object_store = InMemoryObjectStore::new();
    let upload_outcome_store = InMemoryUploadOutcomeStore::new();
    let conversation_summary_store = InMemoryConversationSummaryStore::new();
    let flags_store = InMemoryMessageFlagsStore::new();

    let err = process_upload(
        &object_store,
        &upload_outcome_store,
        &conversation_summary_store,
        &flags_store,
        &user_id,
        upload_id,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        ProcessingError::ObjectStore(ObjectStoreError::NotFound)
    ));
}
