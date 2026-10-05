//! Runs every storage contract against the in-memory fakes -- the same
//! checks the real S3 and DynamoDB adapters must pass (see
//! `s3_object_store.rs`, `dynamo_conversations_table.rs`,
//! `dynamo_message_flags.rs`). If a fake and a real adapter disagree, one
//! of the two suites fails.

#[macro_use]
#[path = "support/object_store_contract.rs"]
mod object_store_contract;
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
#[macro_use]
#[path = "support/message_flags_contract.rs"]
mod message_flags_contract;

use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

mod object_store {
    async fn make() -> (super::InMemoryObjectStore, ()) {
        (super::InMemoryObjectStore::new(), ())
    }
    object_store_contract!(make);
}

mod upload_outcomes {
    async fn make() -> (super::InMemoryUploadOutcomeStore, ()) {
        (super::InMemoryUploadOutcomeStore::new(), ())
    }
    upload_outcome_contract!(make);
}

mod upload_progress {
    async fn make() -> (super::InMemoryUploadOutcomeStore, ()) {
        (super::InMemoryUploadOutcomeStore::new(), ())
    }
    upload_progress_contract!(make);
}

mod upload_received {
    async fn make() -> (super::InMemoryUploadOutcomeStore, ()) {
        (super::InMemoryUploadOutcomeStore::new(), ())
    }
    upload_received_contract!(make);
}

mod conversation_summaries {
    async fn make() -> (super::InMemoryConversationSummaryStore, ()) {
        (super::InMemoryConversationSummaryStore::new(), ())
    }
    conversation_summary_contract!(make);
}

mod message_flags {
    async fn make() -> (super::InMemoryMessageFlagsStore, ()) {
        (super::InMemoryMessageFlagsStore::new(), ())
    }
    message_flags_contract!(make);
}
