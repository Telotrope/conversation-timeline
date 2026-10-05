//! Black-box tests for the `_dev`-only local-testing routes: minting a
//! token via `POST /_dev/login`, uploading through
//! `_dev/local-storage/put/{*key}` (which stores bytes exactly like a real
//! presigned PUT would, and triggers the same processing a real S3 event
//! would), and downloading through `_dev/local-storage/get/{*key}`. All
//! through real HTTP requests against the merged router -- the same shape
//! `main.rs` serves in local dev. See the migration plan's §V2a.

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
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadOutcomeStore;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;
use tower::ServiceExt;

/// Mirrors `main.rs::build_local_state` exactly: the same process-wide
/// generated dev-only keypair, and the same underlying stores shared
/// between `AppState` and `DevState` -- otherwise an upload PUT through the
/// dev router would never be visible to `GET /conversations` on the app
/// router.
fn test_router() -> Router {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());

    // One store for both halves, as the real local server shares it
    // (src/main.rs): POST /uploads records facts that processing reads.
    let upload_outcome_store: Arc<dyn UploadOutcomeStore> =
        Arc::new(InMemoryUploadOutcomeStore::new());

    let app_state = AppState {
        flag_handle_key: Arc::new(FlagHandleKey::generate()),
        object_store: object_store.clone(),
        conversation_summary_store: conversation_summary_store.clone(),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        upload_outcome_store: upload_outcome_store.clone(),
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
    };
    let dev_state = DevState {
        object_store,
        upload_outcome_store: upload_outcome_store.clone(),
        conversation_summary_store,
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store,
        // Nothing here calls POST /_dev/reset, so there is nothing for it to
        // empty. Left explicitly empty rather than wired up, so that a test
        // added later which *does* reset fails loudly instead of quietly
        // clearing nothing.
        resettable: Arc::new(vec![]),
    };
    build_router(app_state).merge(build_dev_router(dev_state))
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn dev_login_mints_a_token_the_real_api_accepts() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    let request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn dev_login_rejects_an_empty_sub() {
    let router = test_router();
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": "" }).to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn putting_a_raw_upload_through_local_storage_triggers_processing() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    // POST /uploads first, exactly like the real fetch()-based upload flow
    // will -- this is what actually produces the upload_url to PUT to.
    let create_request = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            r#"{"file_name":"conversations.json","human_name":"Alice"}"#,
        ))
        .unwrap();
    let create_response = router.clone().oneshot(create_request).await.unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let upload_url = body_json(create_response).await["upload_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(upload_url.starts_with("/_dev/local-storage/put/"));

    let raw = r#"[{"uuid":"11111111-1111-4111-8111-111111111111","name":"Hi","chat_messages":[{"uuid":"22222222-2222-4222-8222-222222222222","sender":"human","created_at":"2024-01-01T00:00:00Z","content":[{"type":"text","text":"WRONG, you failed to fix it."}]}]}]"#;
    let put_request = Request::builder()
        .method("PUT")
        .uri(&upload_url)
        .body(Body::from(raw))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let list_request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let list_response = router.clone().oneshot(list_request).await.unwrap();
    let convs = body_json(list_response).await;
    assert_eq!(convs.as_array().unwrap().len(), 1);
    assert_eq!(convs[0]["name"], "Hi");
    assert_eq!(convs[0]["message_count"], 1);
}

#[tokio::test]
async fn a_non_raw_key_stores_bytes_but_does_not_trigger_processing() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    let put_request = Request::builder()
        .method("PUT")
        .uri("/_dev/local-storage/put/export/alice/whatever.json")
        .body(Body::from("hello"))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let get_request = Request::builder()
        .method("GET")
        .uri("/_dev/local-storage/get/export/alice/whatever.json")
        .body(Body::empty())
        .unwrap();
    let get_response = router.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let bytes = get_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"hello");

    let list_request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let list_response = router.oneshot(list_request).await.unwrap();
    let convs = body_json(list_response).await;
    assert_eq!(convs.as_array().unwrap().len(), 0);
}
