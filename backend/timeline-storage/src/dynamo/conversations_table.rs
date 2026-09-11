//! Real DynamoDB `Conversations` table adapter, implementing both
//! `UploadOutcomeStore` and `ConversationSummaryStore` against one table --
//! the plan's section 1.3 lists this table as holding "conversation/upload
//! metadata"; section 1.6 separately describes writing an "Uploads metadata
//! row." This adapter reconciles the two by keeping both shapes in the same
//! table, distinguished by sort-key prefix: `UPLOAD#<upload_id>` for an
//! upload's own terminal-outcome row (written once, by
//! `record_outcome` -- see the migration plan's §V2a-revision for why there
//! is no earlier, pending row), `CONV#<conversation_id>` for each
//! conversation summary it eventually produces.

use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};

pub struct DynamoConversationsTable {
    client: Client,
    table_name: String,
}

impl DynamoConversationsTable {
    pub fn new(client: Client, table_name: impl Into<String>) -> Self {
        Self {
            client,
            table_name: table_name.into(),
        }
    }
}

fn backend_error(e: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::Backend(Box::new(e))
}

fn invalid_data(msg: impl Into<String>) -> StoreError {
    StoreError::Backend(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        msg.into(),
    )))
}

fn upload_sort_key(upload_id: UploadId) -> String {
    format!("UPLOAD#{upload_id}")
}

fn conversation_sort_key(conversation_id: ConversationId) -> String {
    format!("CONV#{conversation_id}")
}

const CONVERSATION_SORT_PREFIX: &str = "CONV#";

fn upload_outcome_from_item(item: &HashMap<String, AttributeValue>) -> Result<UploadOutcome, StoreError> {
    let status_name = item
        .get("status")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("upload item is missing status"))?;
    match status_name.as_str() {
        "ready" => {
            let ids = item
                .get("conversation_ids")
                .and_then(|v| v.as_l().ok())
                .map(|list| {
                    list.iter()
                        .filter_map(|v| v.as_s().ok())
                        .filter_map(|s| s.parse::<uuid::Uuid>().ok())
                        .map(ConversationId)
                        .collect()
                })
                .unwrap_or_default();
            Ok(UploadOutcome::Ready {
                conversation_ids: ids,
            })
        }
        "failed" => {
            let reason = item
                .get("failure_reason")
                .and_then(|v| v.as_s().ok())
                .cloned()
                .unwrap_or_default();
            Ok(UploadOutcome::Failed { reason })
        }
        other => Err(invalid_data(format!(
            "upload item has unrecognized status {other:?}"
        ))),
    }
}

#[async_trait]
impl UploadOutcomeStore for DynamoConversationsTable {
    async fn record_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        outcome: UploadOutcome,
    ) -> Result<(), StoreError> {
        let mut request = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item("sk", AttributeValue::S(upload_sort_key(upload_id)));
        request = match outcome {
            UploadOutcome::Ready { conversation_ids } => {
                let ids_attr = AttributeValue::L(
                    conversation_ids
                        .iter()
                        .map(|id| AttributeValue::S(id.to_string()))
                        .collect(),
                );
                request
                    .item("status", AttributeValue::S("ready".to_string()))
                    .item("conversation_ids", ids_attr)
            }
            UploadOutcome::Failed { reason } => request
                .item("status", AttributeValue::S("failed".to_string()))
                .item("failure_reason", AttributeValue::S(reason)),
        };
        request.send().await.map_err(backend_error)?;
        Ok(())
    }

    async fn get_outcome(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(upload_sort_key(upload_id)))
            .send()
            .await
            .map_err(backend_error)?;
        output
            .item
            .map(|item| upload_outcome_from_item(&item))
            .transpose()
    }
}

fn conversation_summary_from_item(
    conversation_id: ConversationId,
    item: &HashMap<String, AttributeValue>,
) -> Result<ConversationSummary, StoreError> {
    let upload_id_str = item
        .get("upload_id")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("conversation item is missing upload_id"))?;
    let upload_id = UploadId(
        upload_id_str
            .parse()
            .map_err(|e| invalid_data(format!("bad upload_id: {e}")))?,
    );
    let name = item
        .get("name")
        .and_then(|v| v.as_s().ok())
        .cloned()
        .unwrap_or_default();
    let message_count = item
        .get("message_count")
        .and_then(|v| v.as_n().ok())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(0);
    Ok(ConversationSummary {
        conversation_id,
        upload_id,
        name: ConversationName(name),
        message_count,
    })
}

#[async_trait]
impl ConversationSummaryStore for DynamoConversationsTable {
    async fn list_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let output = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(
                ":prefix",
                AttributeValue::S(CONVERSATION_SORT_PREFIX.to_string()),
            )
            .send()
            .await
            .map_err(backend_error)?;
        output
            .items
            .unwrap_or_default()
            .iter()
            .map(|item| {
                let sk = item
                    .get("sk")
                    .and_then(|v| v.as_s().ok())
                    .ok_or_else(|| invalid_data("conversation item is missing sk"))?;
                let id_str = sk
                    .strip_prefix(CONVERSATION_SORT_PREFIX)
                    .ok_or_else(|| invalid_data(format!("unexpected sk {sk:?}")))?;
                let conversation_id = ConversationId(
                    id_str
                        .parse()
                        .map_err(|e| invalid_data(format!("bad conversation id: {e}")))?,
                );
                conversation_summary_from_item(conversation_id, item)
            })
            .collect()
    }

    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationSummary>, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key(
                "sk",
                AttributeValue::S(conversation_sort_key(conversation_id)),
            )
            .send()
            .await
            .map_err(backend_error)?;
        output
            .item
            .map(|item| conversation_summary_from_item(conversation_id, &item))
            .transpose()
    }

    async fn put(
        &self,
        user_id: &UserId,
        summary: ConversationSummary,
    ) -> Result<(), StoreError> {
        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item(
                "sk",
                AttributeValue::S(conversation_sort_key(summary.conversation_id)),
            )
            .item("upload_id", AttributeValue::S(summary.upload_id.to_string()))
            .item("name", AttributeValue::S(summary.name.0))
            .item(
                "message_count",
                AttributeValue::N(summary.message_count.to_string()),
            )
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
    }
}
