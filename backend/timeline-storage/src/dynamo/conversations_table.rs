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
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore, UploadProgress};

use super::attributes::{
    invalid_data, optional_string, required_count, required_id_list, required_string,
};

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

fn upload_sort_key(upload_id: UploadId) -> String {
    format!("UPLOAD#{upload_id}")
}

/// An upload's attempt count and last error, kept on their own row so the
/// outcome row is never touched before processing finishes.
fn progress_sort_key(upload_id: UploadId) -> String {
    format!("PROGRESS#{upload_id}")
}

fn conversation_sort_key(conversation_id: ConversationId) -> String {
    format!("CONV#{conversation_id}")
}

const CONVERSATION_SORT_PREFIX: &str = "CONV#";

fn upload_outcome_from_item(
    item: &HashMap<String, AttributeValue>,
) -> Result<UploadOutcome, StoreError> {
    match required_string(item, "status")? {
        "ready" => {
            let ids = required_id_list(item, "conversation_ids")?
                .into_iter()
                .map(ConversationId)
                .collect();
            Ok(UploadOutcome::Ready {
                conversation_ids: ids,
            })
        }
        "failed" => {
            let reason = required_string(item, "failure_reason")?.to_string();
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

    async fn record_attempt(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<usize, StoreError> {
        // ADD is applied by DynamoDB itself, so two attempts can't both read
        // the old count; the new count comes back in the same reply, with
        // no separate read to miss it (plan §0b).
        let output = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(progress_sort_key(upload_id)))
            .update_expression("ADD attempts :one")
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::UpdatedNew)
            .send()
            .await
            .map_err(backend_error)?;
        // Unreachable backstop: an `UpdatedNew` reply to this ADD always
        // carries `attempts`.
        let item = output
            .attributes
            .ok_or_else(|| invalid_data("attempt count missing from the update's reply"))?;
        required_count(&item, "attempts")
    }

    async fn record_attempt_error(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        error: String,
    ) -> Result<(), StoreError> {
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(progress_sort_key(upload_id)))
            .update_expression("SET last_error = :error")
            .expression_attribute_values(":error", AttributeValue::S(error))
            .send()
            .await
            .map_err(backend_error)?;
        Ok(())
    }

    async fn get_progress(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadProgress>, StoreError> {
        // Strongly consistent: a default read can miss a row just written
        // (measured: about 1 in 2,000 from inside Lambda, plan §0b).
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(progress_sort_key(upload_id)))
            .consistent_read(true)
            .send()
            .await
            .map_err(backend_error)?;
        let Some(item) = output.item else {
            return Ok(None);
        };
        // A row made only by `record_attempt_error` has no count yet.
        let attempts = match item.get("attempts") {
            None => 0,
            Some(_) => required_count(&item, "attempts")?,
        };
        Ok(Some(UploadProgress {
            attempts,
            last_error: optional_string(&item, "last_error")?.map(str::to_string),
        }))
    }
}

fn conversation_summary_from_item(
    conversation_id: ConversationId,
    item: &HashMap<String, AttributeValue>,
) -> Result<ConversationSummary, StoreError> {
    let upload_id_str = required_string(item, "upload_id")?;
    let upload_id = UploadId(
        upload_id_str
            .parse()
            .map_err(|e| invalid_data(format!("bad upload_id: {e}")))?,
    );
    let name = required_string(item, "name")?.to_string();
    let message_count = required_count(item, "message_count")?;
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
                // Currently unreachable: the table's key schema makes `sk`
                // required, so DynamoDB never returns a row without it.
                // Kept as a backstop if the schema or this read changes.
                let sk = item
                    .get("sk")
                    .and_then(|v| v.as_s().ok())
                    .ok_or_else(|| invalid_data("conversation item is missing sk"))?;
                // Currently unreachable: the query above only asks for rows
                // whose `sk` begins with `CONV#`. Kept as a backstop if the
                // query changes.
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

    async fn put(&self, user_id: &UserId, summary: ConversationSummary) -> Result<(), StoreError> {
        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item(
                "sk",
                AttributeValue::S(conversation_sort_key(summary.conversation_id)),
            )
            .item(
                "upload_id",
                AttributeValue::S(summary.upload_id.to_string()),
            )
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
