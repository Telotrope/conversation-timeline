//! Flag saves through real HTTP requests to the router: the request checks
//! and flag handles from the migration plan's §V2c. A save must name a real
//! message, proven by the handle `GET /export` issued for it, and must
//! actually change something.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use timeline_api::app::{build_dev_router, build_router};
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::dev_state::DevState;
use timeline_api::flag_handles::{FlagHandleKey, FlagHandleKeyError, KEY_ENV_VAR};
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;
use tower::ServiceExt;

/// The repo's real-conversation fixture (6 conversations, trimmed and
/// scoped; see the migration plan's C1).
const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");

fn test_router() -> Router {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());
    let app_state = AppState {
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
        flag_handle_key: Arc::new(FlagHandleKey::generate()),
    };
    let dev_state = DevState {
        object_store,
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        conversation_summary_store,
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store,
        resettable: Arc::new(vec![]),
    };
    build_router(app_state).merge(build_dev_router(dev_state))
}

async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    response.into_body().collect().await.unwrap().to_bytes().to_vec()
}

async fn body_json(response: axum::response::Response) -> Value {
    serde_json::from_slice(&body_bytes(response).await).unwrap()
}

async fn send(router: &Router, request: Request<Body>) -> axum::response::Response {
    router.clone().oneshot(request).await.unwrap()
}

async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    body_json(send(router, request).await).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Uploads `raw` as `user` and returns (token, export reply, exported file).
async fn upload_and_export(router: &Router, user: &str, raw: &str) -> (String, Value, String) {
    let token = dev_login(router, user).await;
    let create = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let upload_url = body_json(send(router, create).await).await["upload_url"]
        .as_str()
        .unwrap()
        .to_string();
    let put = Request::builder()
        .method("PUT")
        .uri(&upload_url)
        .body(Body::from(raw.to_string()))
        .unwrap();
    assert_eq!(send(router, put).await.status(), StatusCode::OK);

    let export = Request::builder()
        .uri("/export")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let reply_bytes = body_bytes(send(router, export).await).await;
    eprintln!(
        "/export reply for {user}: {} bytes",
        reply_bytes.len()
    );
    let reply: Value = serde_json::from_slice(&reply_bytes).unwrap();
    let download = Request::builder()
        .uri(reply["export_url"].as_str().unwrap())
        .body(Body::empty())
        .unwrap();
    let file = String::from_utf8(body_bytes(send(router, download).await).await).unwrap();
    (token, reply, file)
}

/// (conversation id, message id) of every human message in the fixture.
fn human_messages(raw: &str) -> Vec<(String, String)> {
    let parsed: Value = serde_json::from_str(raw).unwrap();
    let mut out = Vec::new();
    for conversation in parsed.as_array().unwrap() {
        for message in conversation["chat_messages"].as_array().unwrap() {
            if message["sender"] == "human" {
                out.push((
                    conversation["uuid"].as_str().unwrap().to_string(),
                    message["uuid"].as_str().unwrap().to_string(),
                ));
            }
        }
    }
    out
}

async fn patch(router: &Router, token: &str, conv: &str, msg: &str, body: Value) -> axum::response::Response {
    patch_raw(router, token, conv, msg, body.to_string()).await
}

async fn patch_raw(router: &Router, token: &str, conv: &str, msg: &str, body: String) -> axum::response::Response {
    let request = Request::builder()
        .method("PATCH")
        .uri(format!("/conversations/{conv}/messages/{msg}/flags"))
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(body))
        .unwrap();
    send(router, request).await
}

fn handle_of<'a>(reply: &'a Value, msg: &str) -> &'a str {
    reply["flag_handles"][msg]
        .as_str()
        .unwrap_or_else(|| panic!("no handle for message {msg}"))
}

async fn assert_status_and_error(
    response: axum::response::Response,
    status: StatusCode,
    mentions: &str,
) {
    assert_eq!(response.status(), status);
    let error = body_json(response).await["error"].as_str().unwrap().to_string();
    assert!(error.contains(mentions), "error {error:?} should mention {mentions:?}");
}

#[tokio::test]
async fn export_issues_a_handle_for_every_human_message_and_only_those() {
    let router = test_router();
    let (_, reply, file) = upload_and_export(&router, "alice", FIXTURE).await;
    let handles = reply["flag_handles"].as_object().unwrap();
    // Compare with the human messages in the exported file itself, not the
    // raw fixture: dedup drops resend duplicates on upload (see the
    // migration plan's C1), and those never reach the page.
    let exported: Value = serde_json::from_str(&file).unwrap();
    let mut exported_humans = std::collections::BTreeSet::new();
    for conversation in exported["conversations"].as_array().unwrap() {
        for message in conversation["chat_messages"].as_array().unwrap() {
            if message["sender"] == "human" {
                exported_humans.insert(message["uuid"].as_str().unwrap().to_string());
            }
        }
    }
    let handle_ids: std::collections::BTreeSet<String> = handles.keys().cloned().collect();
    assert!(!exported_humans.is_empty());
    assert_eq!(handle_ids, exported_humans);
    for handle in handles.values() {
        assert_eq!(handle.as_str().unwrap().len(), 43, "handle length");
    }
    // C20: each entry is a 36-character id, a 43-character handle and JSON
    // punctuation, 85 bytes; printed so the per-message cost is on record.
    eprintln!("/export: {} handles", handles.len());
}

#[tokio::test]
async fn the_exported_file_contains_no_handles() {
    let router = test_router();
    let (_, reply, file) = upload_and_export(&router, "alice", FIXTURE).await;
    assert!(!file.contains("flag_handles"));
    assert!(!file.contains("\"handle\""));
    for handle in reply["flag_handles"].as_object().unwrap().values() {
        assert!(!file.contains(handle.as_str().unwrap()), "a handle leaked into the file");
    }
}

#[tokio::test]
async fn a_save_with_the_issued_handle_succeeds() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let (conv, msg) = human_messages(FIXTURE)
        .into_iter()
        .find(|(_, m)| reply["flag_handles"].get(m).is_some())
        .unwrap();
    let response = patch(
        &router,
        &token,
        &conv,
        &msg,
        json!({"caps": true, "handle": handle_of(&reply, &msg)}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["user"]["caps"], json!(true));
}

/// Two human messages from the fixture that both received handles, in
/// different conversations.
fn two_messages_in_different_conversations(reply: &Value) -> ((String, String), (String, String)) {
    let with_handles: Vec<_> = human_messages(FIXTURE)
        .into_iter()
        .filter(|(_, m)| reply["flag_handles"].get(m).is_some())
        .collect();
    let first = with_handles[0].clone();
    let second = with_handles
        .iter()
        .find(|(c, _)| *c != first.0)
        .expect("fixture has several conversations")
        .clone();
    (first, second)
}

#[tokio::test]
async fn a_handle_for_a_different_message_is_forbidden() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), (_, other_msg)) = two_messages_in_different_conversations(&reply);
    let response = patch(&router, &token, &conv, &msg, json!({"caps": true, "handle": handle_of(&reply, &other_msg)})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn a_real_handle_used_under_a_different_conversation_is_forbidden() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((_, msg), (other_conv, _)) = two_messages_in_different_conversations(&reply);
    let response = patch(&router, &token, &other_conv, &msg, json!({"caps": true, "handle": handle_of(&reply, &msg)})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn a_made_up_message_id_is_forbidden_even_with_a_real_handle() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let made_up = "99999999-9999-4999-8999-999999999999";
    let response = patch(&router, &token, &conv, made_up, json!({"caps": true, "handle": handle_of(&reply, &msg)})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn another_users_handle_is_forbidden() {
    let router = test_router();
    let (_, alice_reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let (bob_token, _, _) = upload_and_export(&router, "bob", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&alice_reply);
    // Same conversation and message ids (bob uploaded the same file), but
    // alice's handle: bob can't use it.
    let response = patch(&router, &bob_token, &conv, &msg, json!({"caps": true, "handle": handle_of(&alice_reply, &msg)})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn a_handle_with_one_character_changed_is_forbidden() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let mut handle = handle_of(&reply, &msg).to_string();
    let last = handle.pop().unwrap();
    handle.push(if last == 'A' { 'B' } else { 'A' });
    let response = patch(&router, &token, &conv, &msg, json!({"caps": true, "handle": handle})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn a_handle_that_is_not_base64_is_forbidden_not_a_server_error() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let response = patch(&router, &token, &conv, &msg, json!({"caps": true, "handle": "not base64 at all!"})).await;
    assert_status_and_error(response, StatusCode::FORBIDDEN, "does not match").await;
}

#[tokio::test]
async fn a_save_without_a_handle_is_a_bad_request() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let response = patch(&router, &token, &conv, &msg, json!({"caps": true})).await;
    assert_status_and_error(response, StatusCode::BAD_REQUEST, "handle").await;
}

#[tokio::test]
async fn a_save_with_nothing_to_change_is_a_bad_request() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let handle = handle_of(&reply, &msg);
    let response = patch(&router, &token, &conv, &msg, json!({"handle": handle})).await;
    assert_status_and_error(response, StatusCode::BAD_REQUEST, "nothing to save").await;
    let response = patch(
        &router,
        &token,
        &conv,
        &msg,
        json!({"handle": handle, "caps": null, "critical": null, "angry": null}),
    )
    .await;
    assert_status_and_error(response, StatusCode::BAD_REQUEST, "nothing to save").await;
}

#[tokio::test]
async fn a_misspelled_field_is_a_bad_request_naming_it() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let response = patch(&router, &token, &conv, &msg, json!({"cap": true, "handle": handle_of(&reply, &msg)})).await;
    assert_status_and_error(response, StatusCode::BAD_REQUEST, "unknown field `cap`").await;
}

/// The 400 message quotes request text (an unknown field's name); it must
/// be cut short rather than echo an arbitrarily long body back.
#[tokio::test]
async fn a_bad_request_message_is_bounded_in_length() {
    let router = test_router();
    let (token, reply, _) = upload_and_export(&router, "alice", FIXTURE).await;
    let ((conv, msg), _) = two_messages_in_different_conversations(&reply);
    let long_field = "x".repeat(5000);
    let body = format!("{{\"{long_field}\": true, \"handle\": \"{}\"}}", handle_of(&reply, &msg));
    let response = patch_raw(&router, &token, &conv, &msg, body).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let error = body_json(response).await["error"].as_str().unwrap().to_string();
    assert!(error.chars().count() <= 301, "error is {} characters", error.chars().count());
    assert!(error.ends_with('…'));
}

// ---- The key itself. The Lambda calls `FlagHandleKey::from_env_value`
// with the environment variable's value at startup and refuses to start on
// an error; these pin what counts as an error.

#[test]
fn a_missing_key_is_refused_with_the_variable_named() {
    let err = FlagHandleKey::from_env_value(None).err().unwrap();
    assert_eq!(err, FlagHandleKeyError::Missing);
    assert!(err.to_string().contains(KEY_ENV_VAR));
}

#[test]
fn a_short_key_is_refused_with_its_length() {
    let err = FlagHandleKey::from_env_value(Some("too-short")).err().unwrap();
    assert_eq!(err, FlagHandleKeyError::TooShort { bytes: 9 });
    assert!(err.to_string().contains(KEY_ENV_VAR));
    assert!(err.to_string().contains("at least 32"));
}

#[test]
fn a_key_from_the_environment_signs_and_checks_consistently() {
    use timeline_core::model::{ConversationId, MessageId};
    use timeline_core::ports::ids::UserId;
    let value = "k".repeat(32);
    let key = FlagHandleKey::from_env_value(Some(&value)).unwrap();
    let same = FlagHandleKey::from_env_value(Some(&value)).unwrap();
    let user = UserId("alice".to_string());
    let conv = ConversationId(uuid::Uuid::from_u128(1));
    let msg = MessageId(uuid::Uuid::from_u128(2));
    // Two Lambda instances given the same key accept each other's handles.
    let handle = key.handle_for(&user, conv, msg);
    assert!(same.verify(&user, conv, msg, &handle));
    // A different key does not.
    let other = FlagHandleKey::from_env_value(Some(&"j".repeat(32))).unwrap();
    assert!(!other.verify(&user, conv, msg, &handle));
}

/// The length prefixes keep "ab" + "c" from signing the same bytes as
/// "a" + "bc". User ids are free text in local dev, so this matters.
#[test]
fn user_ids_that_differ_only_in_where_a_field_boundary_falls_get_different_handles() {
    use timeline_core::model::{ConversationId, MessageId};
    use timeline_core::ports::ids::UserId;
    let key = FlagHandleKey::generate();
    let conv = ConversationId(uuid::Uuid::from_u128(1));
    let msg = MessageId(uuid::Uuid::from_u128(2));
    let a = key.handle_for(&UserId(format!("alice{conv}")), conv, msg);
    let b = key.handle_for(&UserId("alice".to_string()), conv, msg);
    assert_ne!(a, b);
}
