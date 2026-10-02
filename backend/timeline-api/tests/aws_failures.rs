//! Final AWS failures counted apart from retries, through the real storage
//! adapters and the public `aws_call_counter::counting_subscriber`
//! (docs/plans/2026-10-02-activity-instrumentation.md §3, critique C16).
//!
//! Proves the mechanism `timeline_storage::aws_failure` describes: each
//! adapter reports an SDK error that reached it, naming the operation, and
//! the counter turns that into `aws_failures` and `aws_errors` on the
//! current record -- while the SDK's retries before it stay `aws_retries`.

#[path = "../../timeline-storage/tests/support/s3_local.rs"]
mod s3_local;

use std::time::Duration;

use aws_sdk_dynamodb::config::retry::RetryConfig;
use aws_sdk_dynamodb::config::{BehaviorVersion, Credentials, Region};
use timeline_api::aws_call_counter::counting_subscriber;
use timeline_api::request_record::{recording, MAX_KEPT_ERRORS};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::UserId;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

fn failing_dynamodb(addr: std::net::SocketAddr) -> aws_sdk_dynamodb::Client {
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
    aws_sdk_dynamodb::Client::from_conf(config)
}

#[tokio::test]
async fn a_call_that_fails_after_its_retries_is_one_failure_and_two_retries() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let table = DynamoConversationsTable::new(
        failing_dynamodb(always_failing_server().await),
        "conversations",
    );

    let (result, record) = recording(table.list_for_user(&UserId("alice".to_string()))).await;

    assert!(matches!(result, Err(StoreError::Backend(_))), "{result:?}");
    assert_eq!(record.aws_calls.get("DynamoDB.Query"), Some(&1));
    assert_eq!(record.aws_retries, 2);
    assert_eq!(record.aws_failures.get("DynamoDB.Query"), Some(&1));
    assert_eq!(record.aws_failures.len(), 1);
    assert_eq!(record.aws_errors.len(), 1);
    assert!(
        record.aws_errors[0].starts_with("DynamoDB.Query: "),
        "{:?}",
        record.aws_errors
    );
}

#[tokio::test]
async fn only_the_first_few_error_messages_are_kept() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let table = DynamoConversationsTable::new(
        failing_dynamodb(always_failing_server().await),
        "conversations",
    );
    let user = UserId("alice".to_string());

    let ((), record) = recording(async {
        for _ in 0..MAX_KEPT_ERRORS + 2 {
            assert!(table.list_for_user(&user).await.is_err());
        }
    })
    .await;

    assert_eq!(
        record.aws_failures.get("DynamoDB.Query"),
        Some(&(MAX_KEPT_ERRORS as u64 + 2))
    );
    assert_eq!(record.aws_errors.len(), MAX_KEPT_ERRORS);
}

#[tokio::test]
async fn a_missing_object_is_reported_as_a_failed_get_and_still_reads_as_not_found() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let s3 = s3_local::LocalS3::start().await;
    let store = s3.object_store();

    let (result, record) = recording(store.get("raw/nobody/missing.json")).await;

    assert!(
        matches!(result, Err(ObjectStoreError::NotFound)),
        "{result:?}"
    );
    assert_eq!(record.aws_failures.get("S3.GetObject"), Some(&1));
}

#[tokio::test]
async fn a_link_that_cannot_be_signed_is_reported_without_claiming_an_aws_call() {
    let _counting = tracing::subscriber::set_default(counting_subscriber());
    let s3 = s3_local::LocalS3::start().await;
    let store = s3.object_store();
    // AWS's signed links last at most a week; longer is refused when signing.
    let too_long = Duration::from_secs(8 * 24 * 60 * 60);

    let (put, put_record) = recording(store.presign_put("raw/a/b.json", too_long)).await;
    let (get, get_record) = recording(store.presign_get("export/a/b.json", too_long)).await;

    assert!(put.is_err() && get.is_err());
    assert_eq!(
        put_record.aws_failures.get("S3.PutObject presign"),
        Some(&1)
    );
    assert_eq!(
        get_record.aws_failures.get("S3.GetObject presign"),
        Some(&1)
    );
    assert!(
        put_record.aws_calls.is_empty(),
        "signing calls nothing: {:?}",
        put_record.aws_calls
    );
}

#[tokio::test]
async fn without_a_listener_failures_are_still_returned_unchanged() {
    let table = DynamoConversationsTable::new(
        failing_dynamodb(always_failing_server().await),
        "conversations",
    );
    let result = table.list_for_user(&UserId("alice".to_string())).await;
    assert!(matches!(result, Err(StoreError::Backend(_))), "{result:?}");
}
