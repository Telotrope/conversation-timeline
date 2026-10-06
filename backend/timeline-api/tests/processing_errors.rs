//! `process_upload`'s storage errors name what it was saving (plan
//! `2026-10-02-upload-processing-failures.md` §1a). The deployed function
//! once failed with a bare "item not found", which named no row; these check
//! the row and record writes each say which conversation, and how far they
//! got.

#[path = "support/local_app.rs"]
mod local_app;

use std::sync::Arc;

use async_trait::async_trait;
use timeline_api::processing::{process_upload, ProcessingError};
use timeline_api::s3_trigger::ProcessingStores;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::messages::MessageRowWriter;
use timeline_core::ports::uploads::raw_object_key;
use timeline_core::stored_message::{Entry, EntryKey};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::messages::InMemoryMessageStore;

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
    format!(
        "[{}, {}]",
        conversation(CONV_A, MSG_A),
        conversation(CONV_B, MSG_B)
    )
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
        Self {
            inner,
            fail_at,
            calls: std::sync::Mutex::new(0),
        }
    }

    fn this_call_fails(&self) -> bool {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        *calls == self.fail_at
    }
}

/// The row write fails the way DynamoDB's batch writes do when rows are
/// still unwritten after every retry.
#[async_trait]
impl MessageRowWriter for FailingAt<InMemoryMessageStore> {
    async fn put_entries(&self, user_id: &UserId, entries: &[Entry]) -> Result<(), StoreError> {
        if self.this_call_fails() {
            return Err(StoreError::Unwritten {
                left: 1,
                total: entries.len(),
            });
        }
        self.inner.put_entries(user_id, entries).await
    }

    async fn delete_entries(&self, user_id: &UserId, keys: &[EntryKey]) -> Result<(), StoreError> {
        self.inner.delete_entries(user_id, keys).await
    }
}

#[async_trait]
impl ConversationSummaryStore for FailingAt<InMemoryConversationSummaryStore> {
    async fn list_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        self.inner.list_for_user(user_id).await
    }

    async fn list_page(
        &self,
        user_id: &UserId,
        after: Option<ConversationId>,
        max: usize,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        self.inner.list_page(user_id, after, max).await
    }

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError> {
        self.inner.get(user_id, conversation_id).await
    }

    async fn put(
        &self,
        user_id: &UserId,
        summary: ConversationSummary,
    ) -> Result<ConversationSummary, StoreError> {
        if self.this_call_fails() {
            return Err(StoreError::NotFound);
        }
        self.inner.put(user_id, summary).await
    }
}

async fn run(stores: ProcessingStores) -> ProcessingError {
    let user_id = UserId("alice".to_string());
    let upload_id = UploadId(uuid::Uuid::from_u128(1));
    stores
        .object_store
        .put(&raw_object_key(&user_id, upload_id), export().into_bytes())
        .await
        .unwrap();
    local_app::record_upload_facts(&stores, &user_id, upload_id).await;
    process_upload(&stores, &user_id, upload_id)
        .await
        .unwrap_err()
}

/// Replaces `a_failed_review_save_names_the_review_its_number_and_the_total`
/// (plan 2026-10-06-load-only-what-the-page-shows.md §10b): reviews are
/// written with the message rows, in batches, so a failed row write names
/// the conversation, its number and how many rows were left.
#[tokio::test]
async fn a_failed_message_row_write_names_how_many_rows_were_left() {
    let mut stores = local_app::memory_stores();
    stores.message_writer = Arc::new(FailingAt::new(InMemoryMessageStore::new(), 2));
    let err = run(stores).await;

    assert!(
        matches!(
            err,
            ProcessingError::SavingConversation {
                number: 2,
                total: 2,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        err.to_string(),
        format!(
            "saving the message rows of conversation 2 of 2 (conversation {CONV_B}): \
             1 of 1 rows were still unwritten after every retry"
        )
    );
    let source = std::error::Error::source(&err).expect("the store error");
    assert_eq!(
        source.to_string(),
        "1 of 1 rows were still unwritten after every retry"
    );
}

#[tokio::test]
async fn a_failed_summary_save_names_the_conversation_its_number_and_the_total() {
    let mut stores = local_app::memory_stores();
    stores.conversation_summary_store =
        Arc::new(FailingAt::new(InMemoryConversationSummaryStore::new(), 1));
    let err = run(stores).await;

    assert_eq!(
        err.to_string(),
        format!("saving conversation summary 1 of 2 (conversation {CONV_A}): item not found")
    );
    let source = std::error::Error::source(&err).expect("the store error");
    assert_eq!(source.to_string(), "item not found");
}
