//! Black-box test for `GET /export`: upload a real file through the
//! `_dev`-only local flow, run detection, then export it back and confirm
//! the computed flags are embedded in the result -- and that the result
//! re-parses as an already-processed upload, matching
//! `timeline_core::unwrap_uploaded_json`'s two accepted shapes.
//!
//! Detection is an explicit step here because it is an explicit step in the
//! product: uploading no longer computes flags (see
//! `timeline_api::routes::detect`). What this file still owns is whether
//! `GET /export` *embeds* whatever flags exist; whether detection computes
//! the right ones is tests/detect.rs's job.

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
use timeline_core::unwrap_uploaded_json;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;
use tower::ServiceExt;

fn test_router() -> Router {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());

    let app_state = AppState {
        flag_handle_key: Arc::new(FlagHandleKey::generate()),
        object_store: object_store.clone(),
        conversation_summary_store: conversation_summary_store.clone(),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
    };
    let dev_state = DevState {
        object_store,
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
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

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    body_json(response).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn export_embeds_the_auto_flags_that_detection_computed() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    let create_request = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let create_response = router.clone().oneshot(create_request).await.unwrap();
    let upload_url = body_json(create_response).await["upload_url"]
        .as_str()
        .unwrap()
        .to_string();

    let raw = r#"[{"uuid":"11111111-1111-4111-8111-111111111111","name":"Hi","chat_messages":[
        {"uuid":"22222222-2222-4222-8222-222222222222","sender":"human","created_at":"2024-01-01T00:00:00Z","content":[{"type":"text","text":"WRONG, you failed to fix it."}]},
        {"uuid":"33333333-3333-4333-8333-333333333333","sender":"assistant","created_at":"2024-01-01T00:01:00Z","content":[{"type":"text","text":"Sorry, let me retry."}]}
    ]}]"#;
    let put_request = Request::builder()
        .method("PUT")
        .uri(&upload_url)
        .body(Body::from(raw))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    // Flags only exist once they have been asked for.
    let detect_request = Request::builder()
        .method("POST")
        .uri("/detect")
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(json!({ "offset": 0 }).to_string()))
        .unwrap();
    let detect_response = router.clone().oneshot(detect_request).await.unwrap();
    assert_eq!(detect_response.status(), StatusCode::OK);

    let export_request = Request::builder()
        .method("GET")
        .uri("/export")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let export_response = router.clone().oneshot(export_request).await.unwrap();
    assert_eq!(export_response.status(), StatusCode::OK);
    let export_url = body_json(export_response).await["export_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(export_url.starts_with("/_dev/local-storage/get/export/"));

    let download_request = Request::builder()
        .method("GET")
        .uri(&export_url)
        .body(Body::empty())
        .unwrap();
    let download_response = router.oneshot(download_request).await.unwrap();
    assert_eq!(download_response.status(), StatusCode::OK);
    let bytes = body_bytes(download_response).await;
    let text = String::from_utf8(bytes).unwrap();

    // Re-parses as an already-processed upload, per unwrap_uploaded_json's
    // wrapped-object shape.
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    assert!(reparsed.already_processed);
    assert_eq!(reparsed.conversations.len(), 1);

    let human = &reparsed.conversations[0].chat_messages[0];
    let auto = human
        .extra
        .get("_claude_timeline_auto")
        .expect("human message should carry embedded auto flags");
    assert_eq!(auto["caps"], json!(true));
    assert_eq!(auto["critical"], json!(true));

    let assistant = &reparsed.conversations[0].chat_messages[1];
    assert!(
        !assistant.extra.contains_key("_claude_timeline_auto"),
        "assistant messages should never carry auto flags"
    );
}
