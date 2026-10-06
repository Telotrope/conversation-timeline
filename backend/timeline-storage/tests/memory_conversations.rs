//! Black-box tests for `InMemoryConversationSummaryStore`.

use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;

fn user(id: &str) -> UserId {
    UserId(id.to_string())
}

fn summary(n: u128, name: &str) -> ConversationSummary {
    ConversationSummary {
        conversation_id: ConversationId(uuid::Uuid::from_u128(n)),
        name: ConversationName(name.to_string()),
        version: 0,
        source: timeline_core::conversation_metadata::SourceFile {
            upload_id: UploadId(uuid::Uuid::from_u128(1)),
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
    }
}

#[tokio::test]
async fn list_for_user_returns_only_that_users_conversations() {
    let store = InMemoryConversationSummaryStore::new();
    store.insert(user("alice"), summary(1, "alice's chat"));
    store.insert(user("bob"), summary(2, "bob's chat"));

    let alice_convs = store.list_for_user(&user("alice")).await.unwrap();
    assert_eq!(alice_convs.len(), 1);
    assert_eq!(
        alice_convs[0].name,
        ConversationName("alice's chat".to_string())
    );
}

#[tokio::test]
async fn list_for_user_with_no_conversations_is_empty_not_an_error() {
    let store = InMemoryConversationSummaryStore::new();
    assert!(store
        .list_for_user(&user("alice"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn get_a_specific_conversation() {
    let store = InMemoryConversationSummaryStore::new();
    let s = summary(1, "alice's chat");
    store.insert(user("alice"), s.clone());
    assert_eq!(
        store.get(&user("alice"), s.conversation_id).await.unwrap(),
        Some(s)
    );
}

#[tokio::test]
async fn get_a_conversation_that_does_not_exist_is_none() {
    let store = InMemoryConversationSummaryStore::new();
    let missing_id = ConversationId(uuid::Uuid::from_u128(999));
    assert_eq!(store.get(&user("alice"), missing_id).await.unwrap(), None);
}

#[tokio::test]
async fn get_does_not_leak_across_users() {
    let store = InMemoryConversationSummaryStore::new();
    let s = summary(1, "alice's chat");
    store.insert(user("alice"), s.clone());
    assert_eq!(
        store.get(&user("bob"), s.conversation_id).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn put_through_the_trait_method_is_visible_to_list_and_get() {
    let store = InMemoryConversationSummaryStore::new();
    let s = summary(1, "alice's chat");
    let stored = ConversationSummaryStore::put(&store, &user("alice"), s.clone())
        .await
        .unwrap();

    assert_eq!(
        store.get(&user("alice"), s.conversation_id).await.unwrap(),
        Some(stored.clone())
    );
    assert_eq!(
        store.list_for_user(&user("alice")).await.unwrap(),
        vec![stored]
    );
}
