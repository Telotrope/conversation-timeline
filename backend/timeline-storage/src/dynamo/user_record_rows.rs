//! The user's own record (sort key `USER`) and saved analyses (sort key
//! `ANALYSIS#{analysis and options}`) in the `Conversations` table (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c, §8b).
//!
//! The data version is raised with DynamoDB's `ADD`, applied by DynamoDB
//! itself, so two writers can't both read the old value; the new values
//! come back in the same reply.

use async_trait::async_trait;
use aws_sdk_dynamodb::types::{AttributeValue, ReturnValue};
use aws_sdk_dynamodb::Client;
use timeline_core::ports::analyses::AnalysisStore;
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::user_record::{Totals, UserRecord, UserRecordStore};
use timeline_core::server_analyses::{AnalysisKey, SavedAnalysis};

use super::attributes::{invalid_data, json_attribute, required_json, Item};
use super::backend_error;

pub struct DynamoUserRecordStore {
    client: Client,
    table_name: String,
}

impl DynamoUserRecordStore {
    pub fn new(client: Client, table_name: impl Into<String>) -> Self {
        Self {
            client,
            table_name: table_name.into(),
        }
    }
}

const USER_SORT_KEY: &str = "USER";

fn analysis_sort_key(key: &AnalysisKey) -> String {
    format!("ANALYSIS#{key}")
}

/// A whole number of 0 or more; missing means never written.
fn number(item: &Item, name: &str) -> Result<usize, StoreError> {
    match item.get(name) {
        None => Ok(0),
        Some(value) => {
            let text = value.as_n().map_err(|_| {
                invalid_data(format!(
                    "user record: attribute `{name}` should be a number"
                ))
            })?;
            text.parse::<usize>().map_err(|_| {
                invalid_data(format!(
                    "user record: attribute `{name}` should be a whole number of 0 or more but is {text:?}"
                ))
            })
        }
    }
}

fn record_from_item(item: &Item) -> Result<UserRecord, StoreError> {
    Ok(UserRecord {
        data_version: number(item, "data_version")? as u64,
        totals: Totals {
            conversations: number(item, "conversations")?,
            sessions: number(item, "sessions")?,
            your_messages: number(item, "your_messages")?,
            messages: number(item, "messages")?,
        },
    })
}

#[async_trait]
impl UserRecordStore for DynamoUserRecordStore {
    async fn get(&self, user_id: &UserId) -> Result<UserRecord, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(USER_SORT_KEY.to_string()))
            .consistent_read(true)
            .send()
            .await
            .map_err(backend_error("DynamoDB.GetItem"))?;
        output
            .item
            .map_or(Ok(UserRecord::default()), |item| record_from_item(&item))
    }

    async fn raise_version(&self, user_id: &UserId) -> Result<UserRecord, StoreError> {
        self.update(user_id, "ADD data_version :one", &[]).await
    }

    async fn record_totals(
        &self,
        user_id: &UserId,
        totals: Totals,
    ) -> Result<UserRecord, StoreError> {
        let n = |v: usize| AttributeValue::N(v.to_string());
        self.update(
            user_id,
            "ADD data_version :one SET conversations = :c, sessions = :s, your_messages = :y, messages = :m",
            &[
                (":c", n(totals.conversations)),
                (":s", n(totals.sessions)),
                (":y", n(totals.your_messages)),
                (":m", n(totals.messages)),
            ],
        )
        .await
    }
}

impl DynamoUserRecordStore {
    /// Applies `expression` to the user's record and returns it as stored.
    async fn update(
        &self,
        user_id: &UserId,
        expression: &str,
        values: &[(&str, AttributeValue)],
    ) -> Result<UserRecord, StoreError> {
        let mut request = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(USER_SORT_KEY.to_string()))
            .update_expression(expression)
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .return_values(ReturnValue::AllNew);
        for (name, value) in values {
            request = request.expression_attribute_values(*name, value.clone());
        }
        let output = request
            .send()
            .await
            .map_err(backend_error("DynamoDB.UpdateItem"))?;
        // Unreachable backstop: an `AllNew` reply to an update always
        // carries the row.
        let item = output
            .attributes
            .ok_or_else(|| invalid_data("user record missing from the update's reply"))?;
        record_from_item(&item)
    }
}

#[async_trait]
impl AnalysisStore for DynamoUserRecordStore {
    async fn get(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
    ) -> Result<Option<SavedAnalysis>, StoreError> {
        let output = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(user_id.to_string()))
            .key("sk", AttributeValue::S(analysis_sort_key(key)))
            .consistent_read(true)
            .send()
            .await
            .map_err(backend_error("DynamoDB.GetItem"))?;
        output
            .item
            .map(|item| required_json(&item, "saved"))
            .transpose()
    }

    async fn put(
        &self,
        user_id: &UserId,
        key: &AnalysisKey,
        saved: &SavedAnalysis,
    ) -> Result<(), StoreError> {
        self.client
            .put_item()
            .table_name(&self.table_name)
            .item("pk", AttributeValue::S(user_id.to_string()))
            .item("sk", AttributeValue::S(analysis_sort_key(key)))
            .item("saved", json_attribute(saved))
            .send()
            .await
            .map_err(backend_error("DynamoDB.PutItem"))?;
        Ok(())
    }
}
