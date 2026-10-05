//! Conversation metadata through the real routes, on the local server's
//! in-memory stores (plan docs/plans/2026-10-05-screen-flow.md §8):
//! the facts `POST /uploads` records, the guess processing makes from them,
//! recognising conversations an earlier file brought, the export and the
//! scan seeing messages a later file added, and the metadata routes.

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

const CONV_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const CONV_B: &str = "bbbbbbbb-0000-4000-8000-000000000002";

fn router() -> Router {
    router_on(
        Arc::new(InMemoryObjectStore::new()),
        Arc::new(InMemoryConversationSummaryStore::new()),
    )
}

/// The local server over these stores, for tests that damage stored data.
fn router_on(
    objects: Arc<dyn ObjectStore>,
    summaries: Arc<dyn ConversationSummaryStore>,
) -> Router {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags = Arc::new(InMemoryMessageFlagsStore::new());
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

async fn call(
    router: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(b) => builder
            .header("Content-Type", "application/json")
            .body(Body::from(b.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, value)
}

async fn login(router: &Router, sub: &str) -> String {
    let (_, body) = call(
        router,
        "POST",
        "/_dev/login",
        None,
        Some(json!({ "sub": sub })),
    )
    .await;
    body["token"].as_str().unwrap().to_string()
}

/// Uploads `raw` as a file called `file_name`; returns its upload id.
async fn upload(
    router: &Router,
    token: &str,
    file_name: &str,
    written: Option<&str>,
    raw: &Value,
) -> String {
    let mut body = json!({ "file_name": file_name, "human_name": "ada@example.com" });
    if let Some(w) = written {
        body["file_written_at"] = json!(w);
    }
    let (status, created) = call(router, "POST", "/uploads", Some(token), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let url = created["upload_url"].as_str().unwrap();
    let put = Request::builder()
        .method("PUT")
        .uri(url)
        .body(Body::from(raw.to_string()))
        .unwrap();
    assert_eq!(
        router.clone().oneshot(put).await.unwrap().status(),
        StatusCode::OK
    );
    created["upload_id"].as_str().unwrap().to_string()
}

fn message(id: u32, sender: &str, at: &str, text: &str) -> Value {
    json!({
        "uuid": format!("00000000-0000-4000-8000-{id:012}"),
        "sender": sender,
        "created_at": at,
        "content": [{"type": "text", "text": text}],
    })
}

fn conversation(id: &str, name: &str, messages: Vec<Value>) -> Value {
    json!({ "uuid": id, "name": name, "chat_messages": messages })
}

/// Conversation A as a first export holds it: two messages on 1 January.
fn first_a() -> Value {
    conversation(
        CONV_A,
        "A",
        vec![
            message(1, "human", "2026-01-01T10:00:00Z", "Hello"),
            message(2, "assistant", "2026-01-01T10:01:00Z", "Hi"),
        ],
    )
}

/// Conversation A as a later export holds it: the same two, plus two more
/// on 2 January.
fn later_a() -> Value {
    conversation(
        CONV_A,
        "A",
        vec![
            message(1, "human", "2026-01-01T10:00:00Z", "Hello"),
            message(2, "assistant", "2026-01-01T10:01:00Z", "Hi"),
            message(3, "human", "2026-01-02T09:00:00Z", "This is WRONG"),
            message(4, "assistant", "2026-01-02T09:01:00Z", "Sorry"),
        ],
    )
}

async fn conversations(router: &Router, token: &str) -> Vec<Value> {
    let (status, body) = call(router, "GET", "/conversations", Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    body.as_array().unwrap().clone()
}

fn find<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter()
        .find(|c| c["conversation_id"] == id)
        .unwrap_or_else(|| panic!("no conversation {id} in {list:?}"))
}

async fn exported(router: &Router, token: &str) -> Vec<Value> {
    let (status, body) = call(router, "GET", "/export", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let url = body["export_url"].as_str().unwrap().to_string();
    let get = Request::builder().uri(url).body(Body::empty()).unwrap();
    let bytes = router
        .clone()
        .oneshot(get)
        .await
        .unwrap()
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();
    let file: Value = serde_json::from_slice(&bytes).unwrap();
    file["conversations"].as_array().unwrap().clone()
}

fn message_ids(conversation: &Value) -> Vec<String> {
    conversation["chat_messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["uuid"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_new_conversation_gets_the_guess_and_the_file_it_came_in() {
    let router = router();
    let token = login(&router, "alice").await;
    let id = upload(
        &router,
        &token,
        "my export.json",
        Some("2026-01-03T08:00:00Z"),
        &json!([first_a()]),
    )
    .await;
    let list = conversations(&router, &token).await;
    let a = find(&list, CONV_A);
    assert_eq!(a["source"]["upload_id"], id);
    assert_eq!(a["source"]["file_name"], "my export.json");
    assert_eq!(a["source"]["file_written_at"], "2026-01-03T08:00:00Z");
    assert!(a["source"]["uploaded_at"].is_string());
    assert_eq!(
        a["participants"],
        json!([{"kind": "human", "name": "ada@example.com"}, {"kind": "claude"}])
    );
    assert_eq!(a["medium"], json!({"kind": "typed"}));
    assert_eq!(a["details_origin"], "guessed");
    assert_eq!(
        a["span"],
        json!({"start": "2026-01-01T10:00:00Z", "end": "2026-01-01T10:01:00Z"})
    );
    assert_eq!(a["span_origin"], "guessed");
    assert_eq!(a["additions"], json!([]));
    assert_eq!(a["message_count"], 2);
}

#[tokio::test]
async fn starting_an_upload_needs_a_file_name_and_a_human_name() {
    let router = router();
    let token = login(&router, "alice").await;
    for body in [
        json!({ "human_name": "Ada" }),
        json!({ "file_name": "a.json" }),
        json!({ "file_name": " \u{200b} ", "human_name": "Ada" }),
        json!({ "file_name": "a.json", "human_name": "Ada", "colour": "red" }),
    ] {
        let (status, reply) = call(
            &router,
            "POST",
            "/uploads",
            Some(&token),
            Some(body.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body} -> {reply}");
    }
    let (status, _) = call(&router, "POST", "/uploads", Some(&token), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_later_file_adds_only_new_messages_to_a_known_conversation() {
    let router = router();
    let token = login(&router, "alice").await;
    let first = upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    let second = upload(
        &router,
        &token,
        "second.json",
        None,
        &json!([
            later_a(),
            conversation(
                CONV_B,
                "B",
                vec![message(10, "human", "2026-01-05T10:00:00Z", "New one"),]
            )
        ]),
    )
    .await;

    let list = conversations(&router, &token).await;
    assert_eq!(list.len(), 2, "recognised, not added twice: {list:?}");
    let a = find(&list, CONV_A);
    assert_eq!(
        a["source"]["upload_id"], first,
        "keeps the file it first came in"
    );
    assert_eq!(a["additions"], json!([second]));
    assert_eq!(a["message_count"], 4);
    assert_eq!(a["message_span"]["end"], "2026-01-02T09:01:00Z");
    assert_eq!(
        a["span"]["end"], "2026-01-02T09:01:00Z",
        "a guessed span widens"
    );
    assert_eq!(find(&list, CONV_B)["source"]["upload_id"], second);

    let export = exported(&router, &token).await;
    let a = export.iter().find(|c| c["uuid"] == CONV_A).unwrap();
    assert_eq!(
        message_ids(a),
        vec![
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
            "00000000-0000-4000-8000-000000000003",
            "00000000-0000-4000-8000-000000000004",
        ]
    );
}

#[tokio::test]
async fn an_older_file_after_a_newer_one_adds_nothing_and_loses_nothing() {
    let router = router();
    let token = login(&router, "alice").await;
    upload(&router, &token, "newer.json", None, &json!([later_a()])).await;
    upload(&router, &token, "older.json", None, &json!([first_a()])).await;
    let list = conversations(&router, &token).await;
    let a = find(&list, CONV_A);
    assert_eq!(a["additions"], json!([]));
    assert_eq!(a["message_count"], 4);
    let export = exported(&router, &token).await;
    assert_eq!(message_ids(&export[0]).len(), 4);
}

#[tokio::test]
async fn messages_inside_the_stored_range_are_never_added_even_if_new() {
    // The user's rule (plan Q18): only messages outside the stored time
    // range are new. One inside it, missing from the first file, is not
    // added (plan C18 accepts this).
    let router = router();
    let token = login(&router, "alice").await;
    upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    let with_middle = conversation(
        CONV_A,
        "A",
        vec![
            message(1, "human", "2026-01-01T10:00:00Z", "Hello"),
            message(5, "human", "2026-01-01T10:00:30Z", "In between"),
            message(2, "assistant", "2026-01-01T10:01:00Z", "Hi"),
        ],
    );
    upload(&router, &token, "second.json", None, &json!([with_middle])).await;
    let a = find(&conversations(&router, &token).await, CONV_A).clone();
    assert_eq!(a["additions"], json!([]));
    assert_eq!(a["message_count"], 2);
}

#[tokio::test]
async fn a_confirmed_span_is_not_widened_by_a_later_file() {
    let router = router();
    let token = login(&router, "alice").await;
    upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    let span = json!({"start": "2025-12-31T09:00:00+01:00", "end": "2025-12-31T10:00:00+01:00"});
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/conversations/{CONV_A}/metadata"),
        Some(&token),
        Some(json!({"span": span})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    upload(&router, &token, "second.json", None, &json!([later_a()])).await;
    let a = find(&conversations(&router, &token).await, CONV_A).clone();
    assert_eq!(a["span"], span);
    assert_eq!(a["span_origin"], "confirmed");
    assert_eq!(a["message_span"]["end"], "2026-01-02T09:01:00Z");
}

#[tokio::test]
async fn the_scan_reads_messages_a_later_file_added() {
    let router = router();
    let token = login(&router, "alice").await;
    upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    upload(&router, &token, "second.json", None, &json!([later_a()])).await;
    let (status, body) = call(
        &router,
        "POST",
        "/detect",
        Some(&token),
        Some(json!({"offset": 0, "limit": 10})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["messages_detected"], 2,
        "both human messages, one from each file"
    );
    let export = exported(&router, &token).await;
    let added = export[0]["chat_messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["uuid"] == "00000000-0000-4000-8000-000000000003")
        .unwrap();
    assert_eq!(added["_claude_timeline_auto"]["caps"], true, "{added}");
}

#[tokio::test]
async fn the_file_list_counts_each_files_conversations_and_says_varies() {
    let router = router();
    let token = login(&router, "alice").await;
    let first = upload(
        &router,
        &token,
        "first.json",
        Some("2026-01-03T08:00:00Z"),
        &json!([
            first_a(),
            conversation(
                CONV_B,
                "B",
                vec![message(10, "human", "2026-01-05T10:00:00Z", "B one"),]
            )
        ]),
    )
    .await;
    let second = upload(&router, &token, "second.json", None, &json!([later_a()])).await;
    // One of the first file's conversations edited on its own.
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/conversations/{CONV_B}/metadata"),
        Some(&token),
        Some(json!({"participants": [{"kind": "gemini"}]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, files) = call(&router, "GET", "/uploads", Some(&token), None).await;
    assert_eq!(status, StatusCode::OK);
    let files = files.as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["upload_id"], second, "newest first");
    assert_eq!(files[0]["file_name"], "second.json");
    assert_eq!(files[0]["conversation_count"], 0);
    assert_eq!(files[0]["already_present"], 1);
    assert_eq!(files[0]["gained_messages"], 1);
    assert_eq!(
        files[0]["participants"],
        Value::Null,
        "no conversations of its own"
    );
    assert_eq!(files[1]["upload_id"], first);
    assert_eq!(files[1]["file_written_at"], "2026-01-03T08:00:00Z");
    assert_eq!(files[1]["conversation_count"], 2);
    assert_eq!(files[1]["already_present"], 0);
    assert_eq!(files[1]["participants"], Value::Null, "varies");
    assert_eq!(files[1]["medium"], json!({"kind": "typed"}));
    assert_eq!(
        files[1]["details_origin"],
        Value::Null,
        "one confirmed, one guessed"
    );

    let bob = login(&router, "bob").await;
    let (_, none) = call(&router, "GET", "/uploads", Some(&bob), None).await;
    assert_eq!(none, json!([]));
}

#[tokio::test]
async fn a_file_edit_changes_only_the_fields_given_on_every_conversation_of_the_file() {
    let router = router();
    let token = login(&router, "alice").await;
    let first = upload(
        &router,
        &token,
        "first.json",
        None,
        &json!([
            first_a(),
            conversation(
                CONV_B,
                "B",
                vec![message(10, "human", "2026-01-05T10:00:00Z", "B one"),]
            )
        ]),
    )
    .await;
    let (status, _) = call(
        &router,
        "PUT",
        &format!("/conversations/{CONV_B}/metadata"),
        Some(&token),
        Some(json!({"participants": [{"kind": "gemini"}]})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let medium = json!({"kind": "virtual_voice", "transcription": {"service": "zoom"}});
    let (status, changed) = call(
        &router,
        "PUT",
        &format!("/uploads/{first}/metadata"),
        Some(&token),
        Some(json!({"medium": medium})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{changed}");
    assert_eq!(changed.as_array().unwrap().len(), 2);

    let list = conversations(&router, &token).await;
    for id in [CONV_A, CONV_B] {
        assert_eq!(find(&list, id)["medium"], medium);
        assert_eq!(find(&list, id)["details_origin"], "confirmed");
    }
    assert_eq!(
        find(&list, CONV_B)["participants"],
        json!([{"kind": "gemini"}]),
        "left as edited"
    );
    assert_eq!(find(&list, CONV_A)["participants"][0]["kind"], "human");
}

#[tokio::test]
async fn a_file_edit_is_refused_for_spans_nothing_strangers_and_other_peoples_files() {
    let router = router();
    let token = login(&router, "alice").await;
    let first = upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    let uri = format!("/uploads/{first}/metadata");
    let span = json!({"span": {"start": "2026-01-01T00:00:00Z", "end": "2026-01-01T01:00:00Z"}});
    let (status, reply) = call(&router, "PUT", &uri, Some(&token), Some(span)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        reply["error"]
            .as_str()
            .unwrap()
            .contains("one conversation at a time"),
        "{reply}"
    );
    let (status, reply) = call(&router, "PUT", &uri, Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        reply["error"].as_str().unwrap().contains("nothing to save"),
        "{reply}"
    );
    let (status, _) = call(
        &router,
        "PUT",
        &uri,
        Some(&token),
        Some(json!({"participants": []})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let bob = login(&router, "bob").await;
    let (status, _) = call(
        &router,
        "PUT",
        &uri,
        Some(&bob),
        Some(json!({"medium": {"kind": "typed"}})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_conversation_edit_sets_its_start_and_end_and_answers_with_the_record() {
    let router = router();
    let token = login(&router, "alice").await;
    upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    let uri = format!("/conversations/{CONV_A}/metadata");
    let span = json!({"start": "2026-01-01T09:00:00-05:00", "end": "2026-01-01T11:00:00-05:00"});
    let (status, record) = call(
        &router,
        "PUT",
        &uri,
        Some(&token),
        Some(json!({"span": span})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(record["span"], span);
    assert_eq!(record["span_origin"], "confirmed");
    assert_eq!(record["details_origin"], "guessed");

    let backwards =
        json!({"span": {"start": "2026-01-01T11:00:00Z", "end": "2026-01-01T09:00:00Z"}});
    let (status, _) = call(&router, "PUT", &uri, Some(&token), Some(backwards)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let bob = login(&router, "bob").await;
    let (status, _) = call(
        &router,
        "PUT",
        &uri,
        Some(&bob),
        Some(json!({"medium": {"kind": "typed"}})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Stored data our own processing wrote can't normally go bad; when it
/// does, the export fails as a server error (its detail goes to the log,
/// not the page) rather than leaving a conversation out. One case for each
/// way the rebuild can find stored data broken.
#[tokio::test]
async fn damaged_stored_data_is_a_server_error() {
    use timeline_core::ports::ids::{UploadId, UserId};
    use timeline_core::ports::uploads::{addition_object_key, raw_object_key};
    let objects: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let summaries: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());
    let router = router_on(objects.clone(), summaries.clone());
    let token = login(&router, "alice").await;
    let first = upload(&router, &token, "first.json", None, &json!([first_a()])).await;
    upload(&router, &token, "second.json", None, &json!([later_a()])).await;
    let alice = UserId("alice".to_string());
    let first = UploadId(first.parse().unwrap());
    let record = summaries.list_for_user(&alice).await.unwrap().remove(0);
    let added = addition_object_key(&alice, record.conversation_id, record.additions[0]);

    objects.put(&added, b"not json".to_vec()).await.unwrap();
    let (status, _) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    objects
        .put(
            &raw_object_key(&alice, first),
            json!([]).to_string().into_bytes(),
        )
        .await
        .unwrap();
    let (status, _) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    objects
        .put(&raw_object_key(&alice, first), vec![0xff, 0xfe])
        .await
        .unwrap();
    let (status, _) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    objects
        .put(&raw_object_key(&alice, first), b"{".to_vec())
        .await
        .unwrap();
    let (status, _) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}
