//! Black-box tests for `POST /detect`, the user-triggered non-generative
//! detection pass.
//!
//! These carry the assertions that used to live on `process_upload` and on
//! `GET /export` -- that a human message gets real heuristic flags rather
//! than a hardcoded stand-in, and that assistant messages never get an
//! auto-flag record. Detection moved out of upload processing (see
//! `timeline_api::routes::detect`), so the behavior is verified where it now
//! happens rather than deleted along with its old home.

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

/// Text chosen to trip both the caps and criticism heuristics, per
/// timeline-project-decisions.md section 5 -- so an assertion that flags are
/// "real" can point at a specific expected outcome rather than just "some
/// record exists".
const HUMAN_TEXT: &str = "WRONG, you failed to fix it.";

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

/// Uploads `raw` through the real `_dev` local flow and returns the token.
async fn upload(router: &Router, raw: &str) -> String {
    let token = dev_login(router, "alice").await;
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

    let put_request = Request::builder()
        .method("PUT")
        .uri(&upload_url)
        .body(Body::from(raw.to_string()))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);
    token
}

async fn detect(router: &Router, token: &str, body: Value) -> Value {
    let request = Request::builder()
        .method("POST")
        .uri("/detect")
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

async fn export_text(router: &Router, token: &str) -> String {
    let export_request = Request::builder()
        .method("GET")
        .uri("/export")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let export_response = router.clone().oneshot(export_request).await.unwrap();
    let export_url = body_json(export_response).await["export_url"]
        .as_str()
        .unwrap()
        .to_string();
    let download_request = Request::builder()
        .method("GET")
        .uri(&export_url)
        .body(Body::empty())
        .unwrap();
    let download_response = router.clone().oneshot(download_request).await.unwrap();
    let bytes = download_response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    String::from_utf8(bytes).unwrap()
}

fn one_conversation(uuid: &str, name: &str) -> String {
    format!(
        r#"{{"uuid":"{uuid}","name":"{name}","chat_messages":[
            {{"uuid":"22222222-2222-4222-8222-222222222222","sender":"human","created_at":"2024-01-01T00:00:00Z","content":[{{"type":"text","text":"{HUMAN_TEXT}"}}]}},
            {{"uuid":"33333333-3333-4333-8333-333333333333","sender":"assistant","created_at":"2024-01-01T00:01:00Z","content":[{{"type":"text","text":"Sorry, let me retry."}}]}}
        ]}}"#
    )
}

#[tokio::test]
async fn an_upload_has_no_automatic_flags_until_detection_is_asked_for() {
    let router = test_router();
    let raw = format!("[{}]", one_conversation("11111111-1111-4111-8111-111111111111", "Hi"));
    let token = upload(&router, &raw).await;

    // The whole point of the change: uploading is not consent to run a pass
    // over every speech act in the export.
    let text = export_text(&router, &token).await;
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    let human = &reparsed.conversations[0].chat_messages[0];
    assert!(
        !human.extra.contains_key("_claude_timeline_auto"),
        "a freshly uploaded export must carry no automatic flags"
    );
}

#[tokio::test]
async fn detection_computes_the_real_heuristic_flags_not_a_hardcoded_stand_in() {
    let router = test_router();
    let raw = format!("[{}]", one_conversation("11111111-1111-4111-8111-111111111111", "Hi"));
    let token = upload(&router, &raw).await;

    let result = detect(&router, &token, json!({ "offset": 0 })).await;
    assert_eq!(result["messages_detected"], json!(1));
    assert_eq!(result["total_conversations"], json!(1));
    assert_eq!(result["next_offset"], Value::Null);

    let text = export_text(&router, &token).await;
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    let auto = reparsed.conversations[0].chat_messages[0]
        .extra
        .get("_claude_timeline_auto")
        .expect("detected human message should carry embedded auto flags");
    // Specific expected values, not merely "a record exists" -- HUMAN_TEXT is
    // chosen to trip exactly these two.
    assert_eq!(auto["caps"], json!(true), "WRONG should trip the caps heuristic");
    assert_eq!(
        auto["critical"],
        json!(true),
        "'you failed to' should trip the criticism heuristic"
    );
}

#[tokio::test]
async fn detection_never_gives_an_assistant_message_an_auto_flag_record() {
    let router = test_router();
    let raw = format!("[{}]", one_conversation("11111111-1111-4111-8111-111111111111", "Hi"));
    let token = upload(&router, &raw).await;
    detect(&router, &token, json!({ "offset": 0 })).await;

    let text = export_text(&router, &token).await;
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    let assistant = &reparsed.conversations[0].chat_messages[1];
    assert!(
        !assistant.extra.contains_key("_claude_timeline_auto"),
        "assistant messages should never carry auto flags"
    );
}

#[tokio::test]
async fn paging_covers_every_conversation_exactly_once() {
    let router = test_router();
    let raw = format!(
        "[{},{},{}]",
        one_conversation("11111111-1111-4111-8111-111111111111", "One"),
        one_conversation("aaaaaaaa-1111-4111-8111-111111111111", "Two"),
        one_conversation("55555555-1111-4111-8111-111111111111", "Three"),
    );
    let token = upload(&router, &raw).await;

    // Drive the loop the way the page does, one page at a time, and confirm
    // the windows tile the whole set rather than skipping or repeating --
    // the property the route's id-sorting exists to guarantee, given the
    // store itself returns summaries in an arbitrary order.
    let mut offset = 0;
    let mut pages = 0;
    let mut total_detected = 0;
    loop {
        let result = detect(&router, &token, json!({ "offset": offset, "limit": 1 })).await;
        assert_eq!(result["total_conversations"], json!(3));
        assert_eq!(result["conversations_processed"], json!(1));
        total_detected += result["messages_detected"].as_u64().unwrap();
        pages += 1;
        match result["next_offset"].as_u64() {
            Some(next) => offset = next,
            None => break,
        }
        assert!(pages <= 3, "paging did not terminate");
    }
    assert_eq!(pages, 3, "each conversation should be its own page");
    assert_eq!(total_detected, 3, "one human message per conversation");
}
