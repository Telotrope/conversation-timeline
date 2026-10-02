//! Does reading a DynamoDB row straight after writing it miss the row?
//! (plan `docs/plans/2026-10-02-upload-processing-failures.md` §0b.)
//!
//! The deployed processing function failed twice with "item not found" where
//! `set_user_flags` writes a review and reads it back with a default
//! (eventually consistent) read. Run from `dev`, 300 such reads all found
//! their row, but each request there took about 40 ms; from inside Lambda the
//! read follows the write much sooner. [`run`] is the same loop, built to run
//! inside Lambda (`src/bin/read_after_write.rs`).
//!
//! Rows go under one scratch partition key, `experiment#<run id>`, which no
//! user's key can equal (theirs are `<user id>#<conversation id>`), and every
//! row written is deleted at the end.

use std::time::{Duration, Instant};

use aws_sdk_dynamodb::error::DisplayErrorContext;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;
use serde::Serialize;

/// Counts and request times for one kind of read.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct ReadKindReport {
    pub reads: usize,
    pub misses: usize,
    /// Rows whose read missed, by position in the run.
    pub missed_rows: Vec<usize>,
    pub write_median_us: u128,
    pub write_max_us: u128,
    pub read_median_us: u128,
    pub read_max_us: u128,
}

/// What one run found. `error` is the first failed write or read, which
/// stops the run; `delete_errors` are rows the cleanup couldn't delete.
#[derive(Debug, Serialize)]
pub struct Report {
    pub table: String,
    pub partition_key: String,
    pub rows_requested: usize,
    pub default_read: ReadKindReport,
    pub consistent_read: ReadKindReport,
    pub error: Option<String>,
    pub rows_deleted: usize,
    pub delete_errors: Vec<String>,
}

#[derive(Default)]
struct Tally {
    report: ReadKindReport,
    write_times: Vec<Duration>,
    read_times: Vec<Duration>,
}

impl Tally {
    fn finish(mut self) -> ReadKindReport {
        self.write_times.sort();
        self.read_times.sort();
        let median = |v: &[Duration]| v.get(v.len() / 2).map_or(0, Duration::as_micros);
        let max = |v: &[Duration]| v.last().map_or(0, Duration::as_micros);
        self.report.write_median_us = median(&self.write_times);
        self.report.write_max_us = max(&self.write_times);
        self.report.read_median_us = median(&self.read_times);
        self.report.read_max_us = max(&self.read_times);
        self.report
    }
}

/// Writes `rows` new rows to `table`, reading each straight back: even rows
/// with a default read, odd rows with a strongly consistent one. Then deletes
/// every row it wrote. Never panics on an AWS error: errors go in the report.
pub async fn run(client: &Client, table: &str, rows: usize) -> Report {
    let pk = format!("experiment#{}", uuid::Uuid::new_v4());
    let mut default_read = Tally::default();
    let mut consistent_read = Tally::default();
    let mut written = Vec::with_capacity(rows);
    let mut error = None;

    for i in 0..rows {
        let sk = uuid::Uuid::new_v4().to_string();
        // The same kind of request `set_user_flags` sends.
        let started = Instant::now();
        let write = client
            .update_item()
            .table_name(table)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk.clone()))
            .update_expression("SET #user_caps = :user_caps")
            .expression_attribute_names("#user_caps", "user_caps")
            .expression_attribute_values(":user_caps", AttributeValue::Bool(true))
            .send()
            .await;
        let write_time = started.elapsed();
        if let Err(e) = write {
            error = Some(format!(
                "row {i}: write failed: {}",
                DisplayErrorContext(&e)
            ));
            break;
        }
        written.push(sk.clone());

        let consistent = i % 2 == 1;
        let started = Instant::now();
        let read = client
            .get_item()
            .table_name(table)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk))
            .consistent_read(consistent)
            .send()
            .await;
        let read_time = started.elapsed();
        let tally = if consistent {
            &mut consistent_read
        } else {
            &mut default_read
        };
        tally.write_times.push(write_time);
        tally.read_times.push(read_time);
        match read {
            Ok(output) => {
                tally.report.reads += 1;
                if output.item.is_none() {
                    tally.report.misses += 1;
                    tally.report.missed_rows.push(i);
                }
            }
            // Unreachable in tests: a read can only fail after its write
            // succeeded on the same table. Kept as a backstop for real
            // throttling or network errors.
            Err(e) => {
                error = Some(format!("row {i}: read failed: {}", DisplayErrorContext(&e)));
                break;
            }
        }
    }

    let mut delete_errors = Vec::new();
    for sk in &written {
        let deleted = client
            .delete_item()
            .table_name(table)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk.clone()))
            .send()
            .await;
        // Unreachable in tests, for the same reason as the read error above.
        if let Err(e) = deleted {
            delete_errors.push(format!("{sk}: {}", DisplayErrorContext(&e)));
        }
    }

    Report {
        table: table.to_string(),
        partition_key: pk,
        rows_requested: rows,
        default_read: default_read.finish(),
        consistent_read: consistent_read.finish(),
        error,
        rows_deleted: written.len() - delete_errors.len(),
        delete_errors,
    }
}
