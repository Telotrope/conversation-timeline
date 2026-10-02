//! Counting each request's AWS calls, through the public
//! `aws_call_counter::counting_subscriber` and `request_record::recording`
//! (docs/plans/2026-10-02-activity-instrumentation.md §3, critique C5).
//!
//! Proves the mechanism `aws_call_counter` describes against the real AWS
//! SDK: its per-operation spans (`DynamoDB.PutItem`) are counted as calls,
//! its per-attempt spans numbered 2 or more as retries, and nothing else
//! is counted. Real calls go to DynamoDB Local; retries are provoked by a
//! stand-in server that answers every request with 500.

#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;

use std::time::Duration;

use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_dynamodb::types::AttributeValue;
use timeline_api::aws_call_counter::counting_subscriber;
use timeline_api::request_record::{note, recording};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
async fn each_sdk_operation_is_one_call_and_nothing_else_counts() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let client = dynamodb_local::client();
    let table = dynamodb_local::create_table(&client).await;

    let ((), record) = recording(async {
        for n in 0..3 {
            client
                .put_item()
                .table_name(&table)
                .item("pk", AttributeValue::S("p".to_string()))
                .item("sk", AttributeValue::S(format!("s{n}")))
                .send()
                .await
                .unwrap();
        }
        client
            .get_item()
            .table_name(&table)
            .key("pk", AttributeValue::S("p".to_string()))
            .key("sk", AttributeValue::S("s0".to_string()))
            .send()
            .await
            .unwrap();
        // Spans that aren't the SDK's are never counted, even when named
        // like an operation.
        let _other = tracing::debug_span!("Pretend.Operation").entered();
        tracing::info!("an ordinary event");
        note("done", true);
    })
    .await;

    assert_eq!(record.aws_calls.get("DynamoDB.PutItem"), Some(&3));
    assert_eq!(record.aws_calls.get("DynamoDB.GetItem"), Some(&1));
    assert_eq!(record.aws_calls.len(), 2, "{:?}", record.aws_calls);
    assert_eq!(record.aws_retries, 0);
    assert_eq!(record.facts["done"], true);
}

/// A server answering every request with 500 after reading it.
async fn always_failing_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buffer = [0u8; 8192];
                let _bytes_read = socket.read(&mut buffer).await;
                let reply = "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
                let _written = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    addr
}

#[tokio::test]
async fn retried_attempts_are_counted_as_retries_of_one_call() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let addr = always_failing_server().await;
    let config = aws_sdk_dynamodb::Config::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(Credentials::new("test", "test", None, None, "test"))
        .region(Region::new("us-east-1"))
        .endpoint_url(format!("http://{addr}"))
        .retry_config(
            RetryConfig::standard()
                .with_max_attempts(3)
                .with_initial_backoff(Duration::from_millis(1)),
        )
        .build();
    let client = aws_sdk_dynamodb::Client::from_conf(config);

    let (result, record) = recording(client.list_tables().send()).await;

    assert!(result.is_err(), "the stand-in always fails");
    assert_eq!(record.aws_calls.get("DynamoDB.ListTables"), Some(&1));
    assert_eq!(record.aws_retries, 2, "attempts 2 and 3");
}

#[tokio::test]
async fn outside_a_recording_nothing_is_kept_and_nothing_fails() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    note("ignored", 1);
    let client = dynamodb_local::client();
    client.list_tables().send().await.unwrap();

    let ((), record) = recording(async {}).await;
    assert!(record.facts.is_empty());
    assert!(record.aws_calls.is_empty());
}
