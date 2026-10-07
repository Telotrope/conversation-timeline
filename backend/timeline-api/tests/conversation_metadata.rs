//! Conversation metadata through the real routes, on the local server's
//! in-memory stores (plan docs/plans/2026-10-05-screen-flow.md §8):
//! the facts `POST /uploads` records, the guess processing makes from them,
//! recognising conversations an earlier file brought, the export and the
//! scan seeing messages a later file added, and the metadata routes.

#[path = "support/local_app.rs"]
mod local_app;

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use timeline_api::app::{build_dev_router, build_router};
use tower::ServiceExt;

const CONV_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const CONV_B: &str = "bbbbbbbb-0000-4000-8000-000000000002";

fn router() -> Router {
    local_app::router()
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

/// Every record, from inside the reply in parts (plan
/// 2026-10-06-load-only-what-the-page-shows.md §8c).
async fn conversations(router: &Router, token: &str) -> Vec<Value> {
    local_app::conversations(router, token).await
}

fn find<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter()
        .find(|c| c["conversation_id"] == id)
        .unwrap_or_else(|| panic!("no conversation {id} in {list:?}"))
}

/// The annotated download's conversations, joined from its parts.
async fn exported(router: &Router, token: &str) -> Vec<Value> {
    local_app::exported(router, token).await["conversations"]
        .as_array()
        .unwrap()
        .clone()
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
    // Rewritten as approved in plan 2026-10-06-load-only-what-the-page-shows.md
    // §10b: the scan reads message rows, and its request carries a cursor,
    // not an offset; one part holds everything here.
    let (status, body) = call(&router, "POST", "/detect", Some(&token), Some(json!({}))).await;
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
    // The files are inside the reply in parts (§8c); one part here.
    assert_eq!(files["cursor"], Value::Null);
    let files = files["uploads"].as_array().unwrap();
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
    assert_eq!(none["uploads"], json!([]));
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
    // The changed records are inside the reply in parts (§8c).
    assert_eq!(changed["conversations"].as_array().unwrap().len(), 2);

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
/// does, the export and the scan fail as a server error (the detail goes to
/// the log, not the page) rather than leaving a conversation out.
///
/// Rewritten as approved in plan 2026-10-06-load-only-what-the-page-shows.md
/// §10b: both read message rows now, so damage is a damaged row. The
/// in-memory store holds typed rows that can't be damaged, so a reader
/// stands in that reports a damaged row the way the DynamoDB adapter does
/// (a damaged-data error naming the row; see `timeline-storage`'s
/// `tests/dynamo_message_rows.rs`). Plan §12.4: the answer says its kind,
/// `data_integrity`, so the page can say "Data integrity failure".
#[tokio::test]
async fn damaged_stored_rows_are_a_server_error() {
    use timeline_core::model::{ConversationId, MessageId};
    use timeline_core::ports::errors::StoreError;
    use timeline_core::ports::ids::UserId;
    use timeline_core::ports::messages::{EntryRange, MessageReader};
    use timeline_core::stored_message::{Entry, EntryKey};

    struct DamagedRows;

    fn damaged() -> StoreError {
        StoreError::Damaged(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "row with sk \"MSG#...\": attribute `entry` is not the expected JSON",
        )))
    }

    #[async_trait::async_trait]
    impl MessageReader for DamagedRows {
        async fn read_entries(&self, _: &UserId, _: EntryRange) -> Result<Vec<Entry>, StoreError> {
            Err(damaged())
        }
        async fn find_entry(
            &self,
            _: &UserId,
            _: ConversationId,
            _: MessageId,
        ) -> Result<Option<Entry>, StoreError> {
            Err(damaged())
        }
        async fn entry_after(&self, _: &UserId, _: EntryKey) -> Result<Option<Entry>, StoreError> {
            Err(damaged())
        }
    }

    let (healthy, mut state, dev) = local_app::app();
    let token = login(&healthy, "alice").await;
    upload(&healthy, &token, "first.json", None, &json!([first_a()])).await;
    state.message_reader = Arc::new(DamagedRows);
    let router = build_router(state).merge(build_dev_router(dev));

    let (status, body) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(
        body["error"], "stored data can't be read",
        "no detail on the page"
    );
    assert_eq!(body["error_kind"], "data_integrity");
    let (status, _) = call(&router, "POST", "/detect", Some(&token), Some(json!({}))).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

/// Plan §12.4: a store that fails to answer (an outage, not damage) is a
/// server error without the `data_integrity` kind, so the page offers to try
/// again rather than reporting damaged data.
#[tokio::test]
async fn a_store_outage_is_a_server_error_without_a_kind() {
    use timeline_core::model::{ConversationId, MessageId};
    use timeline_core::ports::errors::StoreError;
    use timeline_core::ports::ids::UserId;
    use timeline_core::ports::messages::{EntryRange, MessageReader};
    use timeline_core::stored_message::{Entry, EntryKey};

    struct Down;

    fn down() -> StoreError {
        StoreError::Backend(Box::new(std::io::Error::other("DynamoDB did not answer")))
    }

    #[async_trait::async_trait]
    impl MessageReader for Down {
        async fn read_entries(&self, _: &UserId, _: EntryRange) -> Result<Vec<Entry>, StoreError> {
            Err(down())
        }
        async fn find_entry(
            &self,
            _: &UserId,
            _: ConversationId,
            _: MessageId,
        ) -> Result<Option<Entry>, StoreError> {
            Err(down())
        }
        async fn entry_after(&self, _: &UserId, _: EntryKey) -> Result<Option<Entry>, StoreError> {
            Err(down())
        }
    }

    let (healthy, mut state, dev) = local_app::app();
    let token = login(&healthy, "alice").await;
    upload(&healthy, &token, "first.json", None, &json!([first_a()])).await;
    state.message_reader = Arc::new(Down);
    let router = build_router(state).merge(build_dev_router(dev));

    let (status, body) = call(&router, "GET", "/export", Some(&token), None).await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{body}");
    assert_eq!(body["error"], "storage backend error");
    assert!(body.get("error_kind").is_none(), "{body}");
}
