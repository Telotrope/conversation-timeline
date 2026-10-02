//! `process_upload`'s storage errors name what it was saving (plan
//! `2026-10-02-upload-processing-failures.md` §1a). The deployed function
//! once failed with a bare "item not found", which named no row; these check
//! the review and summary loops each say which one, and how far they got.

use async_trait::async_trait;
use timeline_api::processing::{process_upload, ProcessingError};
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::message_flags::{FlagOverrides, MessageFlagRecord, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::raw_object_key;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

const CONV_A: &str = "11111111-1111-4111-8111-111111111111";
const CONV_B: &str = "44444444-4444-4444-8444-444444444444";
const MSG_A: &str = "22222222-2222-4222-8222-222222222222";
const MSG_B: &str = "55555555-5555-4555-8555-555555555555";

/// Two conversations, one reviewed human message each.
fn export() -> String {
    let conversation = |conv: &str, msg: &str| {
        format!(
            r#"{{"uuid": "{conv}", "name": "c", "chat_messages": [{{
                "uuid": "{msg}", "sender": "human", "created_at": "2024-01-01T00:00:00Z",
                "_claude_timeline_user": {{"critical": true}},
                "content": [{{"type": "text", "text": "hi"}}]}}]}}"#
        )
    };
    format!("[{}, {}]", conversation(CONV_A, MSG_A), conversation(CONV_B, MSG_B))
}

/// Saves through to a real in-memory store until the `fail_at`th call
/// (counting from 1), which fails with `NotFound`.
struct FailingAt<T> {
    inner: T,
    fail_at: usize,
    calls: std::sync::Mutex<usize>,
}

impl<T> FailingAt<T> {
    fn new(inner: T, fail_at: usize) -> Self {
        Self { inner, fail_at, calls: std::sync::Mutex::new(0) }
    }

    fn this_call_fails(&self) -> bool {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        *calls == self.fail_at
    }
}

#[async_trait]
impl UserFlagWriter for FailingAt<InMemoryMessageFlagsStore> {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        overrides: FlagOverrides,
    ) -> Result<MessageFlagRecord, StoreError> {
        if self.this_call_fails() {
            return Err(StoreError::NotFound);
        }
        self.inner.set_user_flags(user_id, conversation_id, message_id, overrides).await
    }
}

#[async_trait]
impl ConversationSummaryStore for FailingAt<InMemoryConversationSummaryStore> {
    async fn list_for_user(&self, user_id: &UserId) -> Result<Vec<ConversationSummary>, StoreError> {
        self.inner.list_for_user(user_id).await
    }

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError> {
        self.inner.get(user_id, conversation_id).await
    }

    async fn put(&self, user_id: &UserId, summary: ConversationSummary) -> Result<(), StoreError> {
        if self.this_call_fails() {
            return Err(StoreError::NotFound);
        }
        self.inner.put(user_id, summary).await
    }
}

async fn run(
    flags: &dyn UserFlagWriter,
    summaries: &dyn ConversationSummaryStore,
) -> ProcessingError {
    let user_id = UserId("alice".to_string());
    let upload_id = UploadId(uuid::Uuid::from_u128(1));
    let objects = InMemoryObjectStore::new();
    objects
        .put(&raw_object_key(&user_id, upload_id), export().into_bytes())
        .await
        .unwrap();
    process_upload(&objects, &InMemoryUploadOutcomeStore::new(), summaries, flags, &user_id, upload_id)
        .await
        .unwrap_err()
}

#[tokio::test]
async fn a_failed_review_save_names_the_review_its_number_and_the_total() {
    let flags = FailingAt::new(InMemoryMessageFlagsStore::new(), 2);
    let err = run(&flags, &InMemoryConversationSummaryStore::new()).await;

    assert!(matches!(err, ProcessingError::SavingReview { number: 2, total: 2, .. }), "{err:?}");
    assert_eq!(
        err.to_string(),
        format!("saving review 2 of 2 (conversation {CONV_B}, message {MSG_B}): item not found")
    );
    let source = std::error::Error::source(&err).expect("the store error");
    assert_eq!(source.to_string(), "item not found");
}

#[tokio::test]
async fn a_failed_summary_save_names_the_conversation_its_number_and_the_total() {
    let summaries = FailingAt::new(InMemoryConversationSummaryStore::new(), 1);
    let err = run(&InMemoryMessageFlagsStore::new(), &summaries).await;

    assert_eq!(
        err.to_string(),
        format!("saving conversation summary 1 of 2 (conversation {CONV_A}): item not found")
    );
    let source = std::error::Error::source(&err).expect("the store error");
    assert_eq!(source.to_string(), "item not found");
}
