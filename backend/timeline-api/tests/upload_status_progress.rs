//! `GET /uploads/{upload_id}` while processing is still running on AWS:
//! which attempt, out of how many, and why the last one failed (plan
//! `2026-10-02-upload-processing-failures.md` §3). Runs the Lambda's router
//! against DynamoDB Local, where the processing Lambda records progress.
//!
//! Needs Java and DynamoDB Local; fails (never skips) without them.

#[path = "support/aws_world.rs"]
#[allow(dead_code)]
mod aws_world;
#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "../../timeline-storage/tests/support/s3_local.rs"]
#[allow(dead_code)]
mod s3_local;

use axum::http::StatusCode;
use serde_json::{json, Value};
use timeline_api::s3_trigger::MAX_PROCESSING_ATTEMPTS;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;

#[tokio::test]
async fn processing_reports_the_attempt_and_the_last_error_until_an_outcome_exists() {
    let world = aws_world::World::new().await;
    let table = DynamoConversationsTable::new(
        world.dynamodb.clone(),
        world.settings.conversations_table.as_str(),
    );
    let router = world.lambda_router();
    let token = world.pool_token("alice");
    let user = UserId("alice".to_string());
    let upload = UploadId(uuid::Uuid::new_v4());
    let status = || async {
        let request = aws_world::get_with(&token, &format!("/uploads/{upload}"));
        let (code, body) = aws_world::call(&router, request).await;
        assert_eq!(code, StatusCode::OK);
        serde_json::from_slice::<Value>(&body).unwrap()
    };

    // Before the first attempt: plain processing, as before.
    assert_eq!(status().await, json!({"status": "processing"}));

    // An error recorded before any attempt counted is shown, with no
    // attempt number.
    table
        .record_attempt_error(&user, upload, "early".to_string())
        .await
        .unwrap();
    assert_eq!(
        status().await,
        json!({"status": "processing", "last_error": "early"})
    );

    table.record_attempt(&user, upload).await.unwrap();
    table.record_attempt(&user, upload).await.unwrap();
    table
        .record_attempt_error(
            &user,
            upload,
            "saving review 2 of 5: item not found".to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        status().await,
        json!({
            "status": "processing",
            "attempt": 2,
            "max_attempts": MAX_PROCESSING_ATTEMPTS,
            "last_error": "saving review 2 of 5: item not found",
        })
    );

    // The outcome wins over any progress.
    table
        .record_outcome(
            &user,
            upload,
            UploadOutcome::Ready {
                conversation_ids: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(status().await, json!({"status": "ready"}));
}
