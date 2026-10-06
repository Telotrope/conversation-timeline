//! Message and note rows in the `Conversations` table (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3): sort key
//! `MSG#{conversation}#{time}#{message}`, so one session's rows are one
//! unbroken run of keys. The time is written at a fixed width (nanoseconds,
//! `Z`), so keys sort as times do; an unknown time is the zero date (§4e).
//!
//! Each row holds the entry as JSON (`entry`, its flags left out), the
//! message id (`message_id`, which finding a message by id filters on), and
//! for your messages `yours = true` and the flags as separate attributes:
//! `auto_caps`, `auto_critical`, `auto_angry` once the scan has run, and
//! `user_caps`, `user_critical`, `user_angry` for each flag you set. The
//! automatic and your writes each name only their own attributes (see
//! [`auto_update`] and [`user_update`]), so neither can overwrite the
//! other's, and both refuse a row that isn't one of your messages rather
//! than creating one.

use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::operation::update_item::UpdateItemError;
use aws_sdk_dynamodb::types::{
    AttributeValue, DeleteRequest, PutRequest, ReturnValue, WriteRequest,
};
use aws_sdk_dynamodb::Client;
use chrono::{DateTime, SecondsFormat, Utc};
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{
    AutoFlagWriter, EntryRange, MessageReader, MessageRowWriter, UserFlagWriter,
};
use timeline_core::stored_message::{Entry, EntryKey};

use super::attributes::{invalid_data, json_attribute, optional_bool, required_json, Item};
use super::backend_error;
use super::batches::write_all;
use super::query::query_all;

pub struct DynamoMessageStore {
    client: Client,
    table_name: String,
}

impl DynamoMessageStore {
    pub fn new(client: Client, table_name: impl Into<String>) -> Self {
        Self {
            client,
            table_name: table_name.into(),
        }
    }
}

const PREFIX: &str = "MSG#";

fn conversation_prefix(conversation_id: ConversationId) -> String {
    format!("{PREFIX}{conversation_id}#")
}

fn time_text(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn sort_key(key: &EntryKey) -> String {
    format!(
        "{}{}#{}",
        conversation_prefix(key.conversation_id),
        time_text(key.at),
        key.id
    )
}

/// Parses a sort key [`sort_key`] wrote; anything else is a backend error
/// naming the key, never skipped.
fn key_from_sort_key(sk: &str) -> Result<EntryKey, StoreError> {
    let bad = || {
        invalid_data(format!(
            "message row sort key {sk:?} is not a conversation, time and id"
        ))
    };
    let rest = sk.strip_prefix(PREFIX).ok_or_else(bad)?;
    let mut parts = rest.splitn(3, '#');
    let (Some(conversation), Some(at), Some(id)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(bad());
    };
    Ok(EntryKey {
        conversation_id: ConversationId(conversation.parse().map_err(|_| bad())?),
        at: DateTime::parse_from_rfc3339(at)
            .map_err(|_| bad())?
            .with_timezone(&Utc),
        id: MessageId(id.parse().map_err(|_| bad())?),
    })
}

fn sk_of(item: &Item) -> Result<&str, StoreError> {
    // Currently unreachable: the table's key schema makes `sk` required, so
    // DynamoDB never returns a row without it.
    item.get("sk")
        .and_then(|v| v.as_s().ok())
        .map(String::as_str)
        .ok_or_else(|| invalid_data("message row is missing its sk"))
}

/// The flags stored on a row of your message.
fn flags_from_item(item: &Item) -> Result<MessageFlags, StoreError> {
    let auto = [
        optional_bool(item, "auto_caps")?,
        optional_bool(item, "auto_critical")?,
        optional_bool(item, "auto_angry")?,
    ];
    Ok(MessageFlags {
        // Written together by the scan: any one present means scanned.
        auto: auto.iter().any(Option::is_some).then(|| FlagSet {
            caps: auto[0].unwrap_or(false),
            critical: auto[1].unwrap_or(false),
            angry: auto[2].unwrap_or(false),
        }),
        user: FlagOverrides {
            caps: optional_bool(item, "user_caps")?,
            critical: optional_bool(item, "user_critical")?,
            angry: optional_bool(item, "user_angry")?,
        },
    })
}

fn entry_from_item(item: &Item) -> Result<Entry, StoreError> {
    let key = key_from_sort_key(sk_of(item)?)?;
    let mut entry: Entry = required_json(item, "entry")?;
    if entry.key() != key {
        return Err(invalid_data(format!(
            "message row {:?} holds an entry with a different key",
            sk_of(item)?
        )));
    }
    if let Entry::Message(message) = &mut entry {
        if optional_bool(item, "yours")? == Some(true) {
            message.flags = Some(flags_from_item(item)?);
        }
    }
    Ok(entry)
}

fn item_for(user_id: &UserId, entry: &Entry) -> Item {
    let key = entry.key();
    let mut item: Item = HashMap::from([
        ("pk".to_string(), AttributeValue::S(user_id.to_string())),
        ("sk".to_string(), AttributeValue::S(sort_key(&key))),
        (
            "message_id".to_string(),
            AttributeValue::S(key.id.to_string()),
        ),
    ]);
    let mut stored = entry.clone();
    if let Entry::Message(message) = &mut stored {
        if let Some(flags) = message.flags.take() {
            item.insert("yours".to_string(), AttributeValue::Bool(true));
            if let Some(auto) = flags.auto {
                item.insert("auto_caps".to_string(), AttributeValue::Bool(auto.caps));
                item.insert(
                    "auto_critical".to_string(),
                    AttributeValue::Bool(auto.critical),
                );
                item.insert("auto_angry".to_string(), AttributeValue::Bool(auto.angry));
            }
            for (name, value) in [
                ("user_caps", flags.user.caps),
                ("user_critical", flags.user.critical),
                ("user_angry", flags.user.angry),
            ] {
                if let Some(v) = value {
                    item.insert(name.to_string(), AttributeValue::Bool(v));
                }
            }
        }
    }
    item.insert("entry".to_string(), json_attribute(&stored));
    item
}

/// One update: its expression, names and values.
struct Update {
    expression: String,
    names: HashMap<String, String>,
    values: HashMap<String, AttributeValue>,
}

/// Sets only the `auto_*` attributes, so a scan can never change your
/// flags.
fn auto_update(flags: FlagSet) -> Update {
    let mut update = Update {
        expression: String::new(),
        names: HashMap::new(),
        values: HashMap::new(),
    };
    let mut sets = Vec::new();
    for (name, value) in [
        ("auto_caps", flags.caps),
        ("auto_critical", flags.critical),
        ("auto_angry", flags.angry),
    ] {
        update.names.insert(format!("#{name}"), name.to_string());
        update
            .values
            .insert(format!(":{name}"), AttributeValue::Bool(value));
        sets.push(format!("#{name} = :{name}"));
    }
    update.expression = format!("SET {}", sets.join(", "));
    update
}

/// Sets only the `user_*` attributes the review names; `None` when it names
/// none, so nothing is sent.
fn user_update(overrides: FlagOverrides) -> Option<Update> {
    let mut update = Update {
        expression: String::new(),
        names: HashMap::new(),
        values: HashMap::new(),
    };
    let mut sets = Vec::new();
    for (name, value) in [
        ("user_caps", overrides.caps),
        ("user_critical", overrides.critical),
        ("user_angry", overrides.angry),
    ] {
        if let Some(v) = value {
            update.names.insert(format!("#{name}"), name.to_string());
            update
                .values
                .insert(format!(":{name}"), AttributeValue::Bool(v));
            sets.push(format!("#{name} = :{name}"));
        }
    }
    if sets.is_empty() {
        return None;
    }
    update.expression = format!("SET {}", sets.join(", "));
    Some(update)
}

impl DynamoMessageStore {
    /// Applies `update` to your message's row under `key`, returning the
    /// row as stored; a key with no such row is `NotFound`, and nothing is
    /// created.
    async fn update_your_row(
        &self,
        user_id: &UserId,
        key: EntryKey,
        mut update: Update,
    ) -> Result<Item, StoreError> {
        update
            .names
            .insert("#yours".to_string(), "yours".to_string());
        update
            .values
            .insert(":true".to_string(), AttributeValue::Bool(true));
        let output = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(sort_key(&key)))
            .update_expression(update.expression)
            .condition_expression("#yours = :true")
            .set_expression_attribute_names(Some(update.names))
            .set_expression_attribute_values(Some(update.values))
            .return_values(ReturnValue::AllNew)
            .send()
            .await
            .map_err(|e| match e.as_service_error() {
                Some(UpdateItemError::ConditionalCheckFailedException(_)) => StoreError::NotFound,
                _ => backend_error("DynamoDB.UpdateItem")(e),
            })?;
        // Unreachable backstop: an `AllNew` reply to a successful update
        // always carries the row.
        output
            .attributes
            .ok_or_else(|| invalid_data("updated row missing from the update's reply"))
    }

    async fn get_row(&self, user_id: &UserId, key: EntryKey) -> Result<Option<Item>, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(sort_key(&key)))
            .consistent_read(true)
            .send()
            .await
            .map_err(backend_error("DynamoDB.GetItem"))?;
        Ok(output.item)
    }
}

#[async_trait]
impl MessageReader for DynamoMessageStore {
    async fn read_entries(
        &self,
        user_id: &UserId,
        range: EntryRange,
    ) -> Result<Vec<Entry>, StoreError> {
        let prefix = conversation_prefix(range.conversation_id);
        let (low, high) = match range.times {
            Some((from, to)) => (
                format!("{prefix}{}", time_text(from)),
                // `~` sorts after every character of a time or an id, so the
                // last message at `to` is included.
                format!("{prefix}{}#~", time_text(to)),
            ),
            None => (prefix.clone(), format!("{prefix}~")),
        };
        let query = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND sk BETWEEN :low AND :high")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(":low", AttributeValue::S(low))
            .expression_attribute_values(":high", AttributeValue::S(high))
            .consistent_read(true);
        let start = range.after.map(|after| {
            HashMap::from([
                ("pk".to_string(), AttributeValue::S(user_id.to_string())),
                ("sk".to_string(), AttributeValue::S(sort_key(&after))),
            ])
        });
        let items = query_all(query, start, None).await?;
        items.iter().map(entry_from_item).collect()
    }

    async fn find_entry(
        &self,
        user_id: &UserId,
        conversation_id: ConversationId,
        message_id: MessageId,
    ) -> Result<Option<Entry>, StoreError> {
        let query = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :prefix)")
            .filter_expression("message_id = :id")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(
                ":prefix",
                AttributeValue::S(conversation_prefix(conversation_id)),
            )
            .expression_attribute_values(":id", AttributeValue::S(message_id.to_string()))
            .consistent_read(true);
        let items = query_all(query, None, Some(1)).await?;
        let mut found = None;
        for item in &items {
            let entry = entry_from_item(item)?;
            if entry.as_message().is_some() {
                found = Some(entry);
                break;
            }
        }
        Ok(found)
    }

    async fn entry_after(
        &self,
        user_id: &UserId,
        key: EntryKey,
    ) -> Result<Option<Entry>, StoreError> {
        let prefix = conversation_prefix(key.conversation_id);
        let query = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression("pk = :pk AND sk BETWEEN :low AND :high")
            .expression_attribute_values(":pk", AttributeValue::S(user_id.to_string()))
            .expression_attribute_values(":low", AttributeValue::S(sort_key(&key)))
            .expression_attribute_values(":high", AttributeValue::S(format!("{prefix}~")))
            .limit(1)
            .consistent_read(true);
        let start = HashMap::from([
            ("pk".to_string(), AttributeValue::S(user_id.to_string())),
            ("sk".to_string(), AttributeValue::S(sort_key(&key))),
        ]);
        let items = query_all(query, Some(start), Some(1)).await?;
        items.first().map(entry_from_item).transpose()
    }
}

#[async_trait]
impl MessageRowWriter for DynamoMessageStore {
    async fn put_entries(&self, user_id: &UserId, entries: &[Entry]) -> Result<(), StoreError> {
        let requests = entries
            .iter()
            .map(|entry| {
                WriteRequest::builder()
                    .put_request(
                        PutRequest::builder()
                            .set_item(Some(item_for(user_id, entry)))
                            .build()
                            // Unreachable backstop: the item is always set.
                            .expect("a put request with an item"),
                    )
                    .build()
            })
            .collect();
        write_all(&self.client, &self.table_name, requests).await
    }

    async fn delete_entries(&self, user_id: &UserId, keys: &[EntryKey]) -> Result<(), StoreError> {
        let requests = keys
            .iter()
            .map(|key| {
                WriteRequest::builder()
                    .delete_request(
                        DeleteRequest::builder()
                            .key("pk", AttributeValue::S(user_id.to_string()))
                            .key("sk", AttributeValue::S(sort_key(key)))
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

#[async_trait]
impl AutoFlagWriter for DynamoMessageStore {
    async fn set_auto_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        flags: FlagSet,
    ) -> Result<(), StoreError> {
        self.update_your_row(user_id, key, auto_update(flags))
            .await
            .map(|_| ())
    }
}

#[async_trait]
impl UserFlagWriter for DynamoMessageStore {
    async fn set_user_flags(
        &self,
        user_id: &UserId,
        key: EntryKey,
        overrides: FlagOverrides,
    ) -> Result<MessageFlags, StoreError> {
        let item = match user_update(overrides) {
            Some(update) => self.update_your_row(user_id, key, update).await?,
            // An empty review changes nothing; it still reports the flags,
            // and a key with no row of yours is not found.
            None => match self.get_row(user_id, key).await? {
                Some(item) if optional_bool(&item, "yours")? == Some(true) => item,
                _ => return Err(StoreError::NotFound),
            },
        };
        flags_from_item(&item)
    }
}
