//! `GET /uploads/{upload_id}`: whether an upload's processing has finished.
//! See the migration plan's §V2e, E3.
//!
//! Most tests run the local build's routers with the stores shared the way
//! `main.rs`'s local branch shares them, so a file PUT through
//! `_dev/local-storage` is processed and its outcome is visible to the
//! route. One runs the Lambda's router against DynamoDB Local.
//!
//! Needs Java and DynamoDB Local for that one test; fails (never skips)
//! without them.

#[path = "support/aws_world.rs"]
#[allow(dead_code)]
mod aws_world;
#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "../../timeline-storage/tests/support/s3_local.rs"]
#[allow(dead_code)]
mod s3_local;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use timeline_api::app::{build_dev_router, build_router};
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::dev_state::DevState;
use timeline_api::flag_handles::FlagHandleKey;
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;
use tower::ServiceExt;

/// The local build's routers, sharing one set of stores as `main.rs`'s
/// `build_local_state` does.
fn local_router() -> Router {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags = Arc::new(InMemoryMessageFlagsStore::new());
    let objects: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let summaries: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());
    let outcomes: Arc<dyn UploadOutcomeStore> = Arc::new(InMemoryUploadOutcomeStore::new());
    let app_state = AppState {
        flag_handle_key: Arc::new(FlagHandleKey::generate()),
        object_store: objects.clone(),
        conversation_summary_store: summaries.clone(),
        flags_reader: flags.clone(),
        user_flag_writer: flags.clone(),
        auto_flag_writer: flags.clone(),
        upload_outcome_store: outcomes.clone(),
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
    };
    let dev_state = DevState {
        object_store: objects,
        upload_outcome_store: outcomes,
        conversation_summary_store: summaries,
        user_flag_writer: flags.clone(),
        auto_flag_writer: flags,
        resettable: Arc::new(vec![]),
    };
    build_router(app_state).merge(build_dev_router(dev_state))
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body)
}

async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    let (status, body) = send(router, request).await;
    assert_eq!(status, StatusCode::OK);
    body["token"].as_str().unwrap().to_string()
}

fn authed(method: &str, uri: &str, token: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(body)
        .unwrap()
}

/// Starts an upload and PUTs `content` to its address; returns the id.
async fn upload(router: &Router, token: &str, content: &str) -> String {
    let (status, created) = send(
        router,
        authed(
            "POST",
            "/uploads",
            token,
            Body::from(r#"{"file_name":"conversations.json","human_name":"Alice"}"#),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let url = created["upload_url"].as_str().unwrap();
    let put = Request::builder()
        .method("PUT")
        .uri(url)
        .body(Body::from(content.to_string()))
        .unwrap();
    let response = router.clone().oneshot(put).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    created["upload_id"].as_str().unwrap().to_string()
}

async fn status_of(router: &Router, token: &str, upload_id: &str) -> (StatusCode, Value) {
    send(
        router,
        authed(
            "GET",
            &format!("/uploads/{upload_id}"),
            token,
            Body::empty(),
        ),
    )
    .await
}

#[tokio::test]
async fn an_upload_with_no_outcome_yet_is_processing() {
    let router = local_router();
    let token = dev_login(&router, "alice").await;
    // Started but never PUT: nothing has processed it.
    let (_, created) = send(
        &router,
        authed(
            "POST",
            "/uploads",
            &token,
            Body::from(r#"{"file_name":"conversations.json","human_name":"Alice"}"#),
        ),
    )
    .await;
    let id = created["upload_id"].as_str().unwrap();
    assert_eq!(
        status_of(&router, &token, id).await,
        (StatusCode::OK, json!({"status": "processing"}))
    );
}

#[tokio::test]
async fn a_processed_upload_is_ready() {
    let router = local_router();
    let token = dev_login(&router, "alice").await;
    let id = upload(&router, &token, aws_world::FIXTURE).await;
    assert_eq!(
        status_of(&router, &token, &id).await,
        (StatusCode::OK, json!({"status": "ready"}))
    );
}

#[tokio::test]
async fn a_file_that_is_not_an_export_is_failed_with_the_reason() {
    let router = local_router();
    let token = dev_login(&router, "alice").await;
    let id = upload(&router, &token, "this is not JSON").await;
    let (status, body) = status_of(&router, &token, &id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], json!("failed"));
    assert!(!body["reason"].as_str().unwrap().is_empty(), "{body}");
}

#[tokio::test]
async fn another_users_upload_looks_like_one_still_processing() {
    let router = local_router();
    let alice = dev_login(&router, "alice").await;
    let bob = dev_login(&router, "bob").await;
    let id = upload(&router, &alice, aws_world::FIXTURE).await;
    assert_eq!(
        status_of(&router, &bob, &id).await,
        (StatusCode::OK, json!({"status": "processing"}))
    );
}

#[tokio::test]
async fn asking_without_a_login_is_refused() {
    let router = local_router();
    let request = Request::builder()
        .uri(format!("/uploads/{}", uuid::Uuid::new_v4()))
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(&router, request).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_upload_id_that_is_not_a_uuid_is_a_bad_request() {
    let router = local_router();
    let token = dev_login(&router, "alice").await;
    let (status, body) = status_of(&router, &token, "not-a-uuid").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("not a UUID"),
        "{body}"
    );
}

/// The Lambda's router reads outcomes from the conversations table, where
/// the processing Lambda writes them.
#[tokio::test]
async fn the_lambda_router_reads_outcomes_from_dynamodb() {
    let world = aws_world::World::new().await;
    let table = DynamoConversationsTable::new(
        world.dynamodb.clone(),
        world.settings.conversations_table.as_str(),
    );
    let user = UserId("alice".to_string());
    let failed = UploadId(uuid::Uuid::new_v4());
    let ready = UploadId(uuid::Uuid::new_v4());
    table
        .record_outcome(
            &user,
            failed,
            UploadOutcome::Failed {
                reason: "not an export".to_string(),
            },
        )
        .await
        .unwrap();
    table
        .record_outcome(
            &user,
            ready,
            UploadOutcome::Ready {
                conversation_ids: vec![],
            },
        )
        .await
        .unwrap();

    let router = world.lambda_router();
    let token = world.pool_token("alice");
    let ask = |id: UploadId| aws_world::get_with(&token, &format!("/uploads/{id}"));
    let (status, body) = aws_world::call(&router, ask(failed)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"status": "failed", "reason": "not an export"})
    );
    let (_, body) = aws_world::call(&router, ask(ready)).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"status": "ready"})
    );
    let (_, body) = aws_world::call(&router, ask(UploadId(uuid::Uuid::new_v4()))).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"status": "processing"})
    );
}
