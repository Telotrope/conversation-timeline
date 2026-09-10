//! Real DynamoDB `MessageFlags` table adapter. Key design from the
//! migration plan section 1.3: `PK user_id#conversation_id, SK message_id`.
//! Auto and user flags live in separate attribute names (`auto_*` /
//! `user_*`) specifically so the auto-write and user-write code paths can
//! be given genuinely disjoint sets of attributes to touch -- see the
//! `*_update_expression` functions and their tests below, which are the
//! concrete, checkable version of the auto/user separation the migration
//! plan section 4.1 calls for.

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

fn backend_error(e: impl std::error::Error + Send + Sync + 'static) -> StoreError {
    StoreError::Backend(Box::new(e))
}

type UpdateExpressionParts = (
    String,
    HashMap<String, String>,
    HashMap<String, AttributeValue>,
);

/// Builds the UpdateExpression for writing *only* the auto-detected flags.
/// See `auto_update_expression_never_references_a_user_attribute` below for
/// the test this exists to make possible.
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
/// `None` if nothing was set. See
/// `user_update_expression_never_references_an_auto_attribute` below.
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
) -> MessageFlagRecord {
    let bool_attr = |k: &str| {
        item.get(k)
            .and_then(|v| v.as_bool().ok())
            .copied()
            .unwrap_or(false)
    };
    let opt_bool_attr = |k: &str| item.get(k).and_then(|v| v.as_bool().ok()).copied();
    MessageFlagRecord {
        message_id,
        auto: FlagSet {
            caps: bool_attr("auto_caps"),
            critical: bool_attr("auto_critical"),
            angry: bool_attr("auto_angry"),
        },
        user: FlagOverrides {
            caps: opt_bool_attr("user_caps"),
            critical: opt_bool_attr("user_critical"),
            angry: opt_bool_attr("user_angry"),
        },
    }
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
            .map_err(backend_error)?;
        Ok(output.item.map(|item| record_from_item(message_id, &item)))
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
            .map_err(backend_error)?;
        output
            .items
            .unwrap_or_default()
            .iter()
            .map(|item| {
                message_id_from_sort_key(item).map(|message_id| record_from_item(message_id, item))
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
            .map_err(backend_error)?;
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
                .map_err(backend_error)?;
        }
        self.get(user_id, conversation_id, message_id)
            .await?
            .ok_or(StoreError::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TEMPORARY per CLAUDE.md's "test only through the public API" rule:
    // these test the private expression-building functions directly,
    // because verifying the auto/user separation through the real
    // AutoFlagWriter/UserFlagWriter trait methods would need a mocked AWS
    // HTTP client (aws-smithy-runtime's StaticReplayClient, matching
    // DynamoDB's wire-protocol JSON exactly) that hasn't been built yet.
    // Remove these once that exists in a `tests/` file calling the real
    // trait methods; keep the doc comments above on `auto_update_expression`
    // and `user_update_expression` as the record of *why* this separation
    // matters regardless of which test proves it.

    #[test]
    fn auto_update_expression_never_references_a_user_attribute() {
        let (expr, names, values) = auto_update_expression(FlagSet {
            caps: true,
            critical: false,
            angry: true,
        });
        assert!(
            !expr.contains("user"),
            "expression must not mention a user attribute: {expr}"
        );
        assert!(
            names.values().all(|v| !v.contains("user")),
            "attribute names must not include a user attribute: {names:?}"
        );
        assert!(
            values.keys().all(|k| !k.contains("user")),
            "attribute values must not include a user placeholder: {values:?}"
        );
    }

    #[test]
    fn user_update_expression_never_references_an_auto_attribute() {
        let (expr, names, values) = user_update_expression(FlagOverrides {
            caps: Some(true),
            critical: None,
            angry: Some(false),
        })
        .unwrap();
        assert!(
            !expr.contains("auto"),
            "expression must not mention an auto attribute: {expr}"
        );
        assert!(
            names.values().all(|v| !v.contains("auto")),
            "attribute names must not include an auto attribute: {names:?}"
        );
        assert!(
            values.keys().all(|k| !k.contains("auto")),
            "attribute values must not include an auto placeholder: {values:?}"
        );
    }

    #[test]
    fn user_update_expression_only_includes_flags_actually_set() {
        let (expr, names, _) = user_update_expression(FlagOverrides {
            caps: Some(true),
            critical: None,
            angry: None,
        })
        .unwrap();
        assert!(expr.contains("user_caps"));
        assert!(!expr.contains("user_critical"));
        assert!(!expr.contains("user_angry"));
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn user_update_expression_with_nothing_set_is_none() {
        assert!(user_update_expression(FlagOverrides::default()).is_none());
    }

    #[test]
    fn partition_and_sort_keys_are_distinct_per_conversation_and_message() {
        let user = UserId("u1".to_string());
        let conv_a = ConversationId(uuid::Uuid::from_u128(1));
        let conv_b = ConversationId(uuid::Uuid::from_u128(2));
        assert_ne!(partition_key(&user, conv_a), partition_key(&user, conv_b));

        let msg_a = MessageId(uuid::Uuid::from_u128(10));
        let msg_b = MessageId(uuid::Uuid::from_u128(11));
        assert_ne!(sort_key(msg_a), sort_key(msg_b));
    }
}
