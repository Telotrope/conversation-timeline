//! Session rows in the `Conversations` table (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3, §6): sort
//! key `SESS#{conversation}#{number}`, the number written with six digits so
//! keys sort as sessions do. The session itself is held as JSON (`session`).

use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::types::{AttributeValue, DeleteRequest, PutRequest, WriteRequest};
use aws_sdk_dynamodb::Client;
use timeline_core::model::ConversationId;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::stored_session::{SessionKey, StoredSession};

use super::attributes::{invalid_data, json_attribute, required_json, Item};
use super::batches::write_all;
use super::query::query_all;

pub struct DynamoSessionStore {
    client: Client,
    table_name: String,
}

impl DynamoSessionStore {
    pub fn new(client: Client, table_name: impl Into<String>) -> Self {
        Self {
            client,
            table_name: table_name.into(),
        }
    }
}

const PREFIX: &str = "SESS#";

fn sort_key(key: SessionKey) -> String {
    format!("{PREFIX}{}#{:06}", key.conversation_id, key.number)
}

fn session_from_item(item: &Item) -> Result<StoredSession, StoreError> {
    let session: StoredSession = required_json(item, "session")?;
    // Currently unreachable: the table's key schema makes `sk` required.
    let sk = item
        .get("sk")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("session row is missing its sk"))?;
    if *sk != sort_key(session.key()) {
        return Err(invalid_data(format!(
            "session row {sk:?} holds a session of a different key"
        )));
    }
    Ok(session)
}

impl DynamoSessionStore {
    async fn query(
        &self,
        user_id: &UserId,
        prefix: String,
        after: Option<SessionKey>,
        max: Option<usize>,
    ) -> Result<Vec<StoredSession>, StoreError> {
        let mut query = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(":prefix", AttributeValue::S(prefix))
            .consistent_read(true);
        if let Some(max) = max {
            query = query.limit(max as i32);
        }
        let start = after.map(|key| {
            HashMap::from([
                ("pk".to_string(), AttributeValue::S(user_id.to_string())),
                ("sk".to_string(), AttributeValue::S(sort_key(key))),
            ])
        });
        let items = query_all(query, start, max).await?;
        items
            .iter()
            .take(max.unwrap_or(usize::MAX))
            .map(session_from_item)
            .collect()
    }
}

#[async_trait]
impl SessionStore for DynamoSessionStore {
    async fn list_sessions(&self, user_id: &UserId) -> Result<Vec<StoredSession>, StoreError> {
        self.query(user_id, PREFIX.to_string(), None, None).await
    }

    async fn sessions_page(
        &self,
        user_id: &UserId,
        after: Option<SessionKey>,
        max: usize,
    ) -> Result<Vec<StoredSession>, StoreError> {
        self.query(user_id, PREFIX.to_string(), after, Some(max.max(1)))
            .await
    }

    async fn sessions_of(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<StoredSession>, StoreError> {
        self.query(user_id, format!("{PREFIX}{conversation_id}#"), None, None)
            .await
    }

    async fn put_sessions(
        &self,
        user_id: &UserId,
        sessions: &[StoredSession],
    ) -> Result<(), StoreError> {
        let requests = sessions
            .iter()
            .map(|session| {
                let item: Item = HashMap::from([
                    ("pk".to_string(), AttributeValue::S(user_id.to_string())),
                    ("sk".to_string(), AttributeValue::S(sort_key(session.key()))),
                    ("session".to_string(), json_attribute(session)),
                ]);
                WriteRequest::builder()
                    .put_request(
                        PutRequest::builder()
                            .set_item(Some(item))
                            .build()
                            // Unreachable backstop: the item is always set.
                            .expect("a put request with an item"),
                    )
                    .build()
            })
            .collect();
        write_all(&self.client, &self.table_name, requests).await
    }

    async fn delete_sessions(
        &self,
        user_id: &UserId,
        keys: &[SessionKey],
    ) -> Result<(), StoreError> {
        let requests = keys
            .iter()
            .map(|key| {
                WriteRequest::builder()
                    .delete_request(
                        DeleteRequest::builder()
                            .key("pk", AttributeValue::S(user_id.to_string()))
                            .key("sk", AttributeValue::S(sort_key(*key)))
                            .build()
                            // Unreachable backstop: the key is always set.
                            .expect("a delete request with a key"),
                    )
                    .build()
            })
            .collect();
        write_all(&self.client, &self.table_name, requests).await
    }
}
