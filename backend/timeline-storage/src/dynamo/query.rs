//! Reading every row a query matches. DynamoDB answers a query at most 1 MB
//! at a time, with a key to carry on from; reading only the first answer
//! silently drops the rest (the cut-off the screen-flow analysis found in
//! `list_for_user`). Every query in these adapters goes through
//! [`query_all`], which carries on until DynamoDB says there is no more.

use std::collections::HashMap;

use aws_sdk_dynamodb::operation::query::builders::QueryFluentBuilder;
use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::ports::errors::StoreError;

use super::attributes::Item;
use super::backend_error;

/// Every row `query` matches, starting after `start` when given; or, with
/// `enough`, stops once at least that many rows have come back.
pub(crate) async fn query_all(
    query: QueryFluentBuilder,
    start: Option<HashMap<String, AttributeValue>>,
    enough: Option<usize>,
) -> Result<Vec<Item>, StoreError> {
    let mut items = Vec::new();
    let mut next = start;
    loop {
        let output = query
            .clone()
            .set_exclusive_start_key(next)
            .send()
            .await
            .map_err(backend_error("DynamoDB.Query"))?;
        items.extend(output.items.unwrap_or_default());
        next = output.last_evaluated_key;
        if next.is_none() || enough.is_some_and(|n| items.len() >= n) {
            return Ok(items);
        }
    }
}
