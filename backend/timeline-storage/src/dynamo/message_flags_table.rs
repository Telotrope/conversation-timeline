//! Real DynamoDB `MessageFlags` table adapter. Key design from the
//! migration plan section 1.3: `PK user_id#conversation_id, SK message_id`.
//! Auto and user flags live in separate attribute names (`auto_*` /
//! `user_*`) specifically so the auto-write and user-write code paths can
//! be given genuinely disjoint sets of attributes to touch -- see the
//! `*_update_expression` functions below, the concrete version of the
//! auto/user separation the migration plan section 4.1 calls for. It is
//! proven through the public trait methods, against DynamoDB Local, by
//! `tests/support/message_flags_contract.rs` (run from
//! `tests/dynamo_message_flags.rs`), which reads back what each kind of
//! write actually stored.

use crate::aws_failure::report;
use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::message_flags::{
    AutoFlagWriter, FlagOverrides, FlagSet, MessageFlagRecord, MessageFlagsReader, UserFlagWriter,
};

use super::attributes::optional_bool;

pub struct DynamoMessageFlagsStore {
    client: Client,
    table_name: String,
}

impl DynamoMessageFlagsStore {
    pub fn new(client: Client, table_name: impl Into<String>) -> Self {
        Self {
            client,
            table_name: table_name.into(),
        }
    }
}

fn partition_key(user_id: &UserId, conversation_id: ConversationId) -> String {
    format!("{user_id}#{conversation_id}")
}

fn sort_key(message_id: MessageId) -> String {
    message_id.to_string()
}

/// Maps a failed call to `operation` to a backend error, reporting it for
/// the request's log line (`crate::aws_failure`).
fn backend_error<E: std::error::Error + Send + Sync + 'static>(
    operation: &'static str,
) -> impl FnOnce(E) -> StoreError {
    move |e| {
        report(operation, &e);
        StoreError::Backend(Box::new(e))
    }
}

type UpdateExpressionParts = (
    String,
    HashMap<String, String>,
    HashMap<String, AttributeValue>,
);

/// Builds the UpdateExpression for writing *only* the auto-detected flags.
/// It names only `auto_*` attributes, so an auto write can never change a
/// user override; the contract test
/// `a_second_auto_write_replaces_the_first_and_keeps_user_overrides`
/// checks that against a real table.
fn auto_update_expression(flags: FlagSet) -> UpdateExpressionParts {
    let names = HashMap::from([
        ("#auto_caps".to_string(), "auto_caps".to_string()),
        ("#auto_critical".to_string(), "auto_critical".to_string()),
        ("#auto_angry".to_string(), "auto_angry".to_string()),
    ]);
    let values = HashMap::from([
        (":auto_caps".to_string(), AttributeValue::Bool(flags.caps)),
        (
            ":auto_critical".to_string(),
            AttributeValue::Bool(flags.critical),
        ),
        (":auto_angry".to_string(), AttributeValue::Bool(flags.angry)),
    ]);
    let expr =
        "SET #auto_caps = :auto_caps, #auto_critical = :auto_critical, #auto_angry = :auto_angry"
            .to_string();
    (expr, names, values)
}

/// Builds the UpdateExpression for writing *only* the flags actually
/// present in `overrides` (a partial PATCH must not clobber the others).
/// `None` if nothing was set, so no write is sent at all. It names only
/// `user_*` attributes, so a user write can never change an auto flag; the
/// contract tests `a_user_write_does_not_disturb_auto_flags`,
/// `a_partial_user_update_only_touches_the_flags_it_names` and
/// `an_empty_user_update_on_an_existing_record_changes_nothing` check that
/// against a real table.
fn user_update_expression(overrides: FlagOverrides) -> Option<UpdateExpressionParts> {
    let mut names = HashMap::new();
    let mut values = HashMap::new();
    let mut sets = Vec::new();
    let mut add = |name: &str, value: bool| {
        names.insert(format!("#{name}"), name.to_string());
        values.insert(format!(":{name}"), AttributeValue::Bool(value));
        sets.push(format!("#{name} = :{name}"));
    };
    if let Some(caps) = overrides.caps {
        add("user_caps", caps);
    }
    if let Some(critical) = overrides.critical {
        add("user_critical", critical);
    }
    if let Some(angry) = overrides.angry {
        add("user_angry", angry);
    }
    if sets.is_empty() {
        return None;
    }
    Some((format!("SET {}", sets.join(", ")), names, values))
}

fn record_from_item(
    message_id: MessageId,
    item: &HashMap<String, AttributeValue>,
) -> Result<MessageFlagRecord, StoreError> {
    // An absent `auto_*` attribute is legitimate and means `false`: a user
    // override can create the row before detection has run. An absent
    // `user_*` attribute means the user hasn't set that flag. Only a value
    // of the wrong type is an error -- see the migration plan's §V2c.
    let auto = |k: &str| optional_bool(item, k).map(|v| v.unwrap_or(false));
    Ok(MessageFlagRecord {
        message_id,
        auto: FlagSet {
            caps: auto("auto_caps")?,
            critical: auto("auto_critical")?,
            angry: auto("auto_angry")?,
        },
        user: FlagOverrides {
            caps: optional_bool(item, "user_caps")?,
            critical: optional_bool(item, "user_critical")?,
            angry: optional_bool(item, "user_angry")?,
        },
    })
}

/// The sort key is always a `MessageId` we ourselves wrote (`sort_key`
/// above) -- if it's ever not, that is a real inconsistency in our own
/// table, not user input, and it must be surfaced as an error, not
/// papered over with a placeholder id.
fn message_id_from_sort_key(
    item: &HashMap<String, AttributeValue>,
) -> Result<MessageId, StoreError> {
    let invalid_data = |msg: String| {
        StoreError::Backend(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            msg,
        )))
    };
    // Currently unreachable: the table's key schema makes `sk` required, so
    // DynamoDB never returns a row without it. Kept as a backstop if the
    // schema or this read changes.
    let sk = item
        .get("sk")
        .and_then(|v| v.as_s().ok())
        .ok_or_else(|| invalid_data("MessageFlags item is missing its sk attribute".to_string()))?;
    let uuid = sk.parse::<uuid::Uuid>().map_err(|e| {
        invalid_data(format!(
            "MessageFlags item sk {sk:?} is not a valid uuid: {e}"
        ))
    })?;
    Ok(MessageId(uuid))
}

#[async_trait]
impl MessageFlagsReader for DynamoMessageFlagsStore {
    async fn get(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<MessageFlagRecord>, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key(
                "pk",
                AttributeValue::S(partition_key(user_id, conversation_id)),
            )
            .key("sk", AttributeValue::S(sort_key(message_id)))
            .send()
            .await
            .map_err(backend_error("DynamoDB.GetItem"))?;
        output
            .item
            .map(|item| record_from_item(message_id, &item))
            .transpose()
    }

    async fn list_for_conversation(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
    ) -> Result<Vec<MessageFlagRecord>, StoreError> {
        let output = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk")
            .expression_attribute_values(
                ":pk",
                AttributeValue::S(partition_key(user_id, conversation_id)),
            )
            .send()
            .await
            .map_err(backend_error("DynamoDB.Query"))?;
        output
            .items
            .unwrap_or_default()
            .iter()
            .map(|item| {
                message_id_from_sort_key(item)
                    .and_then(|message_id| record_from_item(message_id, item))
            })
            .collect()
    }
}

#[async_trait]
impl AutoFlagWriter for DynamoMessageFlagsStore {
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        flags: FlagSet,
    ) -> Result<(), StoreError> {
        let (expr, names, values) = auto_update_expression(flags);
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key(
                "pk",
                AttributeValue::S(partition_key(user_id, conversation_id)),
            )
            .key("sk", AttributeValue::S(sort_key(message_id)))
            .update_expression(expr)
            .set_expression_attribute_names(Some(names))
            .set_expression_attribute_values(Some(values))
            .send()
            .await
            .map_err(backend_error("DynamoDB.UpdateItem"))?;
        Ok(())
    }
}

#[async_trait]
impl UserFlagWriter for DynamoMessageFlagsStore {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
        overrides: FlagOverrides,
    ) -> Result<MessageFlagRecord, StoreError> {
        if let Some((expr, names, values)) = user_update_expression(overrides) {
            self.client
                .update_item()
                .table_name(&self.table_name)
                .key(
                    "pk",
                    AttributeValue::S(partition_key(user_id, conversation_id)),
                )
                .key("sk", AttributeValue::S(sort_key(message_id)))
                .update_expression(expr)
                .set_expression_attribute_names(Some(names))
                .set_expression_attribute_values(Some(values))
                .send()
                .await
                .map_err(backend_error("DynamoDB.UpdateItem"))?;
        }
        self.get(user_id, conversation_id, message_id)
            .await?
            .ok_or(StoreError::NotFound)
    }
}
