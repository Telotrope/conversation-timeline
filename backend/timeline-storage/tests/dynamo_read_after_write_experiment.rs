//! An experiment, not a regression test (plan
//! `docs/plans/2026-10-02-upload-processing-failures.md` §0): does reading a
//! row straight after writing it miss the row on the real DynamoDB table?
//! The deployed processing function failed twice with "item not found" where
//! `set_user_flags` writes a review and reads it back with a default
//! (eventually consistent) read. DynamoDB Local always reads its own writes,
//! so only the real table can answer.
//!
//! Skipped by ordinary test runs (`#[ignore]`): it costs money, needs AWS
//! credentials, and writes to a deployed table. Run on demand:
//!
//! ```text
//! aws login --profile timeline
//! AWS_PROFILE=timeline AWS_REGION=us-east-1 \
//! TIMELINE_EXPERIMENT_TABLE=timeline-message-flags-dev \
//!   cargo test -p timeline-storage --test dynamo_read_after_write_experiment -- --ignored --nocapture
//! ```
//!
//! Rows go under one scratch partition key, `experiment#<run id>`, which no
//! user's key can equal (theirs are `<user id>#<conversation id>`), and are
//! all deleted at the end.

use std::time::{Duration, Instant};

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;

const ROWS: usize = 600;

#[derive(Default)]
struct Tally {
    reads: usize,
    misses: usize,
    write_times: Vec<Duration>,
    read_times: Vec<Duration>,
}

impl Tally {
    fn report(&mut self, label: &str) {
        self.write_times.sort();
        self.read_times.sort();
        let median = |v: &[Duration]| v[v.len() / 2];
        let max = |v: &[Duration]| v[v.len() - 1];
        println!(
            "{label}: {} reads, {} missed the row just written; \
             write median {:?} max {:?}; read median {:?} max {:?}",
            self.reads,
            self.misses,
            median(&self.write_times),
            max(&self.write_times),
            median(&self.read_times),
            max(&self.read_times),
        );
    }
}

#[tokio::test]
#[ignore = "writes to a real DynamoDB table; run on demand, see the module doc"]
async fn reads_straight_after_a_write_on_the_real_table() {
    let table = std::env::var("TIMELINE_EXPERIMENT_TABLE")
        .expect("TIMELINE_EXPERIMENT_TABLE must name the deployed flags table");
    let config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let client = Client::new(&config);
    let pk = format!("experiment#{}", uuid::Uuid::new_v4());
    println!("table {table}, partition key {pk}, {ROWS} rows");

    let mut default_read = Tally::default();
    let mut consistent_read = Tally::default();
    let mut written = Vec::with_capacity(ROWS);
    let mut first_error = None;

    for i in 0..ROWS {
        let sk = uuid::Uuid::new_v4().to_string();
        // The same kind of request `set_user_flags` sends.
        let started = Instant::now();
        let write = client
            .update_item()
            .table_name(&table)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk.clone()))
            .update_expression("SET #user_caps = :user_caps")
            .expression_attribute_names("#user_caps", "user_caps")
            .expression_attribute_values(":user_caps", AttributeValue::Bool(true))
            .send()
            .await;
        let write_time = started.elapsed();
        if let Err(e) = write {
            first_error = Some(format!("write {i} failed: {e:?}"));
            break;
        }
        written.push(sk.clone());

        let consistent = i % 2 == 1;
        let started = Instant::now();
        let read = client
            .get_item()
            .table_name(&table)
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
                tally.reads += 1;
                if output.item.is_none() {
                    tally.misses += 1;
                    println!(
                        "row {i} ({}): missed",
                        if consistent { "consistent" } else { "default" }
                    );
                }
            }
            Err(e) => {
                first_error = Some(format!("read {i} failed: {e:?}"));
                break;
            }
        }
    }

    if default_read.reads > 0 {
        default_read.report("default reads");
    }
    if consistent_read.reads > 0 {
        consistent_read.report("strongly consistent reads");
    }

    let mut delete_failures = Vec::new();
    for sk in &written {
        if let Err(e) = client
            .delete_item()
            .table_name(&table)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S(sk.clone()))
            .send()
            .await
        {
            delete_failures.push(format!("{sk}: {e:?}"));
        }
    }
    println!(
        "deleted {} of {} rows",
        written.len() - delete_failures.len(),
        written.len()
    );

    assert!(first_error.is_none(), "{}", first_error.unwrap_or_default());
    assert!(
        delete_failures.is_empty(),
        "rows left under {pk}: {delete_failures:?}"
    );
}
