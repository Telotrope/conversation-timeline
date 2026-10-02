//! `read_after_write::run` against DynamoDB Local, which always reads its own
//! writes, so these check the loop's bookkeeping, not DynamoDB's behavior.

#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;

use timeline_experiments::read_after_write::run;

#[tokio::test]
async fn every_row_is_read_back_counted_by_kind_and_deleted() {
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;

    let report = run(&client, &table, 7).await;

    assert_eq!(report.error, None);
    assert_eq!(report.rows_requested, 7);
    assert_eq!(report.default_read.reads, 4);
    assert_eq!(report.consistent_read.reads, 3);
    assert_eq!(report.default_read.misses, 0);
    assert!(report.default_read.missed_rows.is_empty());
    assert!(report.default_read.write_max_us >= report.default_read.write_median_us);
    assert!(report.consistent_read.read_max_us >= report.consistent_read.read_median_us);
    assert!(report.partition_key.starts_with("experiment#"));
    assert_eq!(report.rows_deleted, 7);
    assert!(report.delete_errors.is_empty());
    let left = client.scan().table_name(&table).send().await.expect("scan");
    assert_eq!(left.count(), 0, "rows left behind");
}

#[tokio::test]
async fn a_missing_table_stops_at_the_first_write_and_says_so() {
    let client = dynamodb_local::client();

    let report = run(&client, "no-such-table", 5).await;

    let error = report.error.clone().expect("an error");
    assert!(error.starts_with("row 0: write failed:"), "{error}");
    assert_eq!(report.default_read.reads, 0);
    assert_eq!(report.default_read.write_median_us, 0);
    assert_eq!(report.rows_deleted, 0);

    let json = serde_json::to_value(&report).expect("serializes");
    assert_eq!(json["table"], "no-such-table");
}
