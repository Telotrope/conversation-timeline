//! `aws_call_counter::install`, as the Lambda binaries call it at start-up:
//! afterwards, AWS calls made anywhere in the process are counted
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3). In its own test
//! file because installing is once per process.

#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;

use timeline_api::aws_call_counter::install;
use timeline_api::request_record::recording;

#[tokio::test(flavor = "multi_thread")]
async fn once_installed_calls_on_any_thread_are_counted() {
    install();
    let client = dynamodb_local::client();
    let (result, record) = recording(client.list_tables().send()).await;
    result.unwrap();
    assert_eq!(record.aws_calls.get("DynamoDB.ListTables"), Some(&1));
}
