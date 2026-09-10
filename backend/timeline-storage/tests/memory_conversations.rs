//! Black-box tests for `InMemoryConversationStore`.

use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationStore, ConversationSummary};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_storage::memory::conversations::InMemoryConversationStore;

fn user(id: &str) -> UserId {
    UserId(id.to_string())
}

fn summary(n: u128, name: &str) -> ConversationSummary {
    ConversationSummary {
        conversation_id: ConversationId(uuid::Uuid::from_u128(n)),
        upload_id: UploadId(uuid::Uuid::from_u128(1)),
        name: ConversationName(name.to_string()),
        message_count: 3,
    }
}

#[tokio::test]
async fn list_for_user_returns_only_that_users_conversations() {
    let store = InMemoryConversationStore::new();
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
    let store = InMemoryConversationStore::new();
    assert!(store
        .list_for_user(&user("alice"))
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn get_a_specific_conversation() {
    let store = InMemoryConversationStore::new();
    let s = summary(1, "alice's chat");
    store.insert(user("alice"), s.clone());
    assert_eq!(
        store.get(&user("alice"), s.conversation_id).await.unwrap(),
        Some(s)
    );
}

#[tokio::test]
async fn get_a_conversation_that_does_not_exist_is_none() {
    let store = InMemoryConversationStore::new();
    let missing_id = ConversationId(uuid::Uuid::from_u128(999));
    assert_eq!(store.get(&user("alice"), missing_id).await.unwrap(), None);
}

#[tokio::test]
async fn get_does_not_leak_across_users() {
    let store = InMemoryConversationStore::new();
    let s = summary(1, "alice's chat");
    store.insert(user("alice"), s.clone());
    assert_eq!(
        store.get(&user("bob"), s.conversation_id).await.unwrap(),
        None
    );
}
