//! Real DynamoDB `Conversations` table adapter, implementing both
//! `UploadOutcomeStore` and `ConversationSummaryStore` against one table --
//! the plan's section 1.3 lists this table as holding "conversation/upload
//! metadata"; section 1.6 separately describes writing an "Uploads metadata
//! row." This adapter reconciles the two by keeping both shapes in the same
//! table, distinguished by sort-key prefix: `UPLOAD#<upload_id>` for an
//! upload's own terminal-outcome row (written once, by
//! `record_outcome` -- see the migration plan's §V2a-revision for why there
//! is no earlier, pending row), `CONV#<conversation_id>` for each
//! conversation summary it eventually produces, and `RECEIVED#<upload_id>`
//! for what `POST /uploads` learned about the file (its name, when it was
//! uploaded and last written, the human's name). The same table holds the
//! sessions, message rows, the user's record and saved analyses, each under
//! its own prefix (see `crate::dynamo`).
//!
//! A conversation record is written only if its stored `version` is still
//! the one the writer read (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7, C1): a
//! DynamoDB condition on the write, so two files processed at once can't
//! silently replace each other's changes.

use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::operation::put_item::PutItemError;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use timeline_core::conversation_metadata::UploadFacts;
use timeline_core::model::{ConversationId, ConversationName};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{
    ProcessingProgress, UploadOutcome, UploadOutcomeStore, UploadProgress,
};

use super::attributes::{
    invalid_data, json_attribute, optional_string, required_count, required_id_list, required_json,
    required_string,
};
use super::backend_error;
use super::query::query_all;

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

fn upload_sort_key(upload_id: UploadId) -> String {
    format!("UPLOAD#{upload_id}")
}

/// An upload's attempt count and last error, kept on their own row so the
/// outcome row is never touched before processing finishes.
fn progress_sort_key(upload_id: UploadId) -> String {
    format!("PROGRESS#{upload_id}")
}

/// What `POST /uploads` recorded about the file, read back by processing.
fn received_sort_key(upload_id: UploadId) -> String {
    format!("RECEIVED#{upload_id}")
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
        request
            .send()
            .await
            .map_err(backend_error("DynamoDB.PutItem"))?;
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
            .map_err(backend_error("DynamoDB.GetItem"))?;
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
            .map_err(backend_error("DynamoDB.UpdateItem"))?;
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
            .map_err(backend_error("DynamoDB.UpdateItem"))?;
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
            .map_err(backend_error("DynamoDB.GetItem"))?;
        let Some(item) = output.item else {
            return Ok(None);
        };
        // A row made only by `record_attempt_error` has no count yet.
        let attempts = match item.get("attempts") {
            None => 0,
            Some(_) => required_count(&item, "attempts")?,
        };
        let processing = match item.get("processing") {
            None => None,
            Some(_) => Some(required_json(&item, "processing")?),
        };
        Ok(Some(UploadProgress {
            attempts,
            last_error: optional_string(&item, "last_error")?.map(str::to_string),
            processing,
        }))
    }

    async fn record_processing_progress(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        progress: ProcessingProgress,
    ) -> Result<(), StoreError> {
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(progress_sort_key(upload_id)))
            .update_expression("SET processing = :processing")
            .expression_attribute_values(":processing", json_attribute(&progress))
            .send()
            .await
            .map_err(backend_error("DynamoDB.UpdateItem"))?;
        Ok(())
    }

    async fn record_received(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
        facts: UploadFacts,
    ) -> Result<(), StoreError> {
        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item("sk", AttributeValue::S(received_sort_key(upload_id)))
            .item("facts", json_attribute(&facts))
            .send()
            .await
            .map_err(backend_error("DynamoDB.PutItem"))?;
        Ok(())
    }

    async fn get_received(
        &self,
        user_id: &UserId,
        upload_id: UploadId,
    ) -> Result<Option<UploadFacts>, StoreError> {
        // Strongly consistent: processing reads this right after
        // `POST /uploads` wrote it (see `get_progress`).
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(received_sort_key(upload_id)))
            .consistent_read(true)
            .send()
            .await
            .map_err(backend_error("DynamoDB.GetItem"))?;
        output
            .item
            .map(|item| required_json(&item, "facts"))
            .transpose()
    }
}

fn conversation_summary_from_item(
    conversation_id: ConversationId,
    item: &HashMap<String, AttributeValue>,
) -> Result<ConversationSummary, StoreError> {
    let name = required_string(item, "name")?.to_string();
    let message_count = required_count(item, "message_count")?;
    // A row written before conversations had metadata (2026-10-05) has an
    // `upload_id` and no `source`. Nothing it holds says what its file was
    // called or when it came, and inventing those would show made-up facts
    // as real, so it is refused with a message saying why.
    if !item.contains_key("source") && item.contains_key("upload_id") {
        return Err(invalid_data(format!(
            "conversation {conversation_id} was stored before conversations had metadata \
             (plan 2026-10-05-screen-flow.md); it has no source file, so it must be \
             uploaded again after the old rows are cleared"
        )));
    }
    // Fields added later are read last, so a row damaged in an older field
    // is reported as that.
    let source = required_json(item, "source")?;
    let additions = required_json(item, "additions")?;
    let message_span = required_json(item, "message_span")?;
    let participants = required_json(item, "participants")?;
    let medium = required_json(item, "medium")?;
    let details_origin = required_json(item, "details_origin")?;
    let span = required_json(item, "span")?;
    let span_origin = required_json(item, "span_origin")?;
    Ok(ConversationSummary {
        conversation_id,
        name: ConversationName(name),
        version: required_count(item, "version")? as u64,
        source,
        additions,
        message_count,
        untimed: required_count(item, "untimed")?,
        message_span,
        participants,
        medium,
        details_origin,
        span,
        span_origin,
        branch_of: required_json(item, "branch_of")?,
        branches: required_json(item, "branches")?,
    })
}

/// A conversation record from a row of the `CONV#` prefix.
fn summary_from_row(
    item: &HashMap<String, AttributeValue>,
) -> Result<ConversationSummary, StoreError> {
    // Currently unreachable: the table's key schema makes `sk` required, so
    // DynamoDB never returns a row without it. Kept as a backstop if the
    // schema or this read changes.
    let sk = item
        .get("sk")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("conversation item is missing sk"))?;
    // Currently unreachable: every query here only asks for rows whose `sk`
    // begins with `CONV#`. Kept as a backstop if a query changes.
    let id_str = sk
        .strip_prefix(CONVERSATION_SORT_PREFIX)
        .ok_or_else(|| invalid_data(format!("unexpected sk {sk:?}")))?;
    let conversation_id = ConversationId(
        id_str
            .parse()
            .map_err(|e| invalid_data(format!("bad conversation id: {e}")))?,
    );
    conversation_summary_from_item(conversation_id, item)
}

impl DynamoConversationsTable {
    fn conversations_query(
        &self,
        user_id: &UserId,
    ) -> aws_sdk_dynamodb::operation::query::builders::QueryFluentBuilder {
        self.client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(
                ":prefix",
                AttributeValue::S(CONVERSATION_SORT_PREFIX.to_string()),
            )
            .consistent_read(true)
    }
}

#[async_trait]
impl ConversationSummaryStore for DynamoConversationsTable {
    async fn list_for_user(
        &self,
        user_id: &UserId,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let items = query_all(self.conversations_query(user_id), None, None).await?;
        items.iter().map(summary_from_row).collect()
    }

    async fn list_page(
        &self,
        user_id: &UserId,
        after: Option<ConversationId>,
        max: usize,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let max = max.max(1);
        let start = after.map(|id| {
            HashMap::from([
                ("pk".to_string(), AttributeValue::S(user_id.to_string())),
                (
                    "sk".to_string(),
                    AttributeValue::S(conversation_sort_key(id)),
                ),
            ])
        });
        let query = self.conversations_query(user_id).limit(max as i32);
        let items = query_all(query, start, Some(max)).await?;
        items.iter().take(max).map(summary_from_row).collect()
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
            .map_err(backend_error("DynamoDB.GetItem"))?;
        output
            .item
            .map(|item| conversation_summary_from_item(conversation_id, &item))
            .transpose()
    }

    async fn put(
        &self,
        user_id: &UserId,
        mut summary: ConversationSummary,
    ) -> Result<ConversationSummary, StoreError> {
        let read_version = summary.version;
        summary.version += 1;
        let mut request = self
            .client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item(
                "sk",
                AttributeValue::S(conversation_sort_key(summary.conversation_id)),
            )
            .item("name", AttributeValue::S(summary.name.0.clone()))
            .item("version", AttributeValue::N(summary.version.to_string()))
            .item("source", json_attribute(&summary.source))
            .item("additions", json_attribute(&summary.additions))
            .item(
                "message_count",
                AttributeValue::N(summary.message_count.to_string()),
            )
            .item("untimed", AttributeValue::N(summary.untimed.to_string()))
            .item("message_span", json_attribute(&summary.message_span))
            .item("participants", json_attribute(&summary.participants))
            .item("medium", json_attribute(&summary.medium))
            .item("details_origin", json_attribute(&summary.details_origin))
            .item("span", json_attribute(&summary.span))
            .item("span_origin", json_attribute(&summary.span_origin))
            .item("branch_of", json_attribute(&summary.branch_of))
            .item("branches", json_attribute(&summary.branches));
        request = if read_version == 0 {
            request.condition_expression("attribute_not_exists(sk)")
        } else {
            request
                .condition_expression("version = :read")
                .expression_attribute_values(":read", AttributeValue::N(read_version.to_string()))
        };
        request
            .send()
            .await
            .map_err(|e| match e.as_service_error() {
                Some(PutItemError::ConditionalCheckFailedException(_)) => StoreError::Conflict,
                _ => backend_error("DynamoDB.PutItem")(e),
            })?;
        Ok(summary)
    }
}
