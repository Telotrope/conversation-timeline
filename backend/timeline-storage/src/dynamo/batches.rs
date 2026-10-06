//! Writing many rows quickly (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7). DynamoDB
//! takes at most 25 rows per `BatchWriteItem`; sent one after another at
//! 10–30 ms each, the 4,800 rows of a large export would take seconds. So up
//! to [`AT_ONCE`] batches are in flight at a time.
//!
//! DynamoDB may finish only part of a batch, returning the rest as
//! unprocessed. Those are resent, after a pause that doubles each time, up
//! to [`TRIES`] tries in all. Rows still unwritten then are reported as
//! [`StoreError::Unwritten`], naming how many were left of how many, and the
//! caller's attempt fails (on AWS, S3's retry of processing runs it again).

use std::time::Duration;

use aws_sdk_dynamodb::types::WriteRequest;
use aws_sdk_dynamodb::Client;
use futures::stream::{self, StreamExt};
use timeline_core::ports::errors::StoreError;

use crate::aws_failure::report;

/// DynamoDB's own limit on rows per batch.
pub(crate) const BATCH_ROWS: usize = 25;
/// Batches in flight at a time.
pub(crate) const AT_ONCE: usize = 16;
/// Sends of one batch, the first included, before its leftover rows count
/// as unwritten.
pub(crate) const TRIES: u32 = 5;
const FIRST_PAUSE: Duration = Duration::from_millis(50);

/// Writes (puts or deletes) every request; see the module doc.
pub(crate) async fn write_all(
    client: &Client,
    table_name: &str,
    requests: Vec<WriteRequest>,
) -> Result<(), StoreError> {
    let total = requests.len();
    let chunks: Vec<Vec<WriteRequest>> = requests
        .chunks(BATCH_ROWS)
        .map(|chunk| chunk.to_vec())
        .collect();
    let results: Vec<Result<usize, StoreError>> = stream::iter(chunks)
        .map(|chunk| write_one_batch(client, table_name, chunk))
        .buffer_unordered(AT_ONCE)
        .collect()
        .await;
    let mut left = 0;
    for result in results {
        left += result?;
    }
    if left > 0 {
        return Err(StoreError::Unwritten { left, total });
    }
    Ok(())
}

/// Sends one batch until DynamoDB has taken all of it or the tries run
/// out; returns how many rows were still unwritten.
async fn write_one_batch(
    client: &Client,
    table_name: &str,
    mut pending: Vec<WriteRequest>,
) -> Result<usize, StoreError> {
    let mut pause = FIRST_PAUSE;
    for try_number in 1..=TRIES {
        let output = client
            .batch_write_item()
            .request_items(table_name, pending)
            .send()
            .await
            .map_err(|e| {
                report("DynamoDB.BatchWriteItem", &e);
                StoreError::Backend(Box::new(e))
            })?;
        pending = output
            .unprocessed_items
            .and_then(|mut items| items.remove(table_name))
            .unwrap_or_default();
        if pending.is_empty() {
            return Ok(0);
        }
        if try_number < TRIES {
            tokio::time::sleep(pause).await;
            pause *= 2;
        }
    }
    Ok(pending.len())
}
