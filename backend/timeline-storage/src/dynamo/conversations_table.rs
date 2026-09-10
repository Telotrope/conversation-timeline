//! Real DynamoDB `Conversations` table adapter, implementing both
//! `UploadStore` and `ConversationStore` against one table -- the plan's
//! section 1.3 lists this table as holding "conversation/upload metadata";
//! section 1.6 separately describes writing an "Uploads metadata row." This
//! adapter reconciles the two by keeping both shapes in the same table,
//! distinguished by sort-key prefix: `UPLOAD#<upload_id>` for an upload's
//! own status row, `CONV#<conversation_id>` for each conversation summary
//! it eventually produces.

use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationStore, ConversationSummary};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadRecord, UploadStatus, UploadStore};

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

fn upload_record_from_item(
    upload_id: UploadId,
    item: &HashMap<String, AttributeValue>,
) -> Result<UploadRecord, StoreError> {
    let raw_object_key = item
        .get("raw_object_key")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("upload item is missing raw_object_key"))?
        .clone();
    let status_name = item
        .get("status")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("upload item is missing status"))?;
    let status = match status_name.as_str() {
        "pending" => UploadStatus::Pending,
        "processing" => UploadStatus::Processing,
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
            UploadStatus::Ready {
                conversation_ids: ids,
            }
        }
        "failed" => {
            let reason = item
                .get("failure_reason")
                .and_then(|v| v.as_s().ok())
                .cloned()
                .unwrap_or_default();
            UploadStatus::Failed { reason }
        }
        other => {
            return Err(invalid_data(format!(
                "upload item has unrecognized status {other:?}"
            )))
        }
    };
    Ok(UploadRecord {
        upload_id,
        status,
        raw_object_key,
    })
}

#[async_trait]
impl UploadStore for DynamoConversationsTable {
    async fn create_pending(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        raw_object_key: &str,
    ) -> Result<(), StoreError> {
        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item("sk", AttributeValue::S(upload_sort_key(upload_id)))
            .item("status", AttributeValue::S("pending".to_string()))
            .item(
                "raw_object_key",
                AttributeValue::S(raw_object_key.to_string()),
            )
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn get(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadRecord>, StoreError> {
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
            .map(|item| upload_record_from_item(upload_id, &item))
            .transpose()
    }

    async fn mark_processing(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<(), StoreError> {
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(upload_sort_key(upload_id)))
            .update_expression("SET #status = :status")
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":status", AttributeValue::S("processing".to_string()))
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn mark_ready(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        conversation_ids: Vec<ConversationId>,
    ) -> Result<(), StoreError> {
        let ids_attr = AttributeValue::L(
            conversation_ids
                .iter()
                .map(|id| AttributeValue::S(id.to_string()))
                .collect(),
        );
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(upload_sort_key(upload_id)))
            .update_expression("SET #status = :status, conversation_ids = :ids")
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":status", AttributeValue::S("ready".to_string()))
            .expression_attribute_values(":ids", ids_attr)
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn mark_failed(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        reason: String,
    ) -> Result<(), StoreError> {
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(upload_sort_key(upload_id)))
            .update_expression("SET #status = :status, failure_reason = :reason")
            .expression_attribute_names("#status", "status")
            .expression_attribute_values(":status", AttributeValue::S("failed".to_string()))
            .expression_attribute_values(":reason", AttributeValue::S(reason))
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
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
impl ConversationStore for DynamoConversationsTable {
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

    async fn create(
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
