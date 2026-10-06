//! Black-box tests for `POST /detect`, the user-triggered non-generative
//! detection pass.
//!
//! These carry the assertions that used to live on `process_upload` and on
//! `GET /export` -- that a human message gets real heuristic flags rather
//! than a hardcoded stand-in, and that assistant messages never get an
//! auto-flag record. Detection moved out of upload processing (see
//! `timeline_api::routes::detect`), so the behavior is verified where it now
//! happens rather than deleted along with its old home.

#[path = "support/local_app.rs"]
mod local_app;

use axum::http::StatusCode;
use axum::Router;
use serde_json::{json, Value};
use timeline_core::unwrap_uploaded_json;

/// Text chosen to trip both the caps and criticism heuristics, per
/// timeline-project-decisions.md section 5 -- so an assertion that flags are
/// "real" can point at a specific expected outcome rather than just "some
/// record exists".
const HUMAN_TEXT: &str = "WRONG, you failed to fix it.";

/// Uploads `raw` through the real `_dev` local flow and returns the token.
async fn upload(router: &Router, raw: &str) -> String {
    local_app::signed_in_with(router, "alice", raw).await
}

async fn detect(router: &Router, token: &str, body: Value) -> Value {
    let (status, reply) = local_app::send(
        router,
        local_app::request("POST", "/detect", token, Some(body)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    reply
}

async fn export_text(router: &Router, token: &str) -> String {
    local_app::export(router, token).await.0
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
    let router = local_app::router();
    let raw = format!(
        "[{}]",
        one_conversation("11111111-1111-4111-8111-111111111111", "Hi")
    );
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
    let router = local_app::router();
    let raw = format!(
        "[{}]",
        one_conversation("11111111-1111-4111-8111-111111111111", "Hi")
    );
    let token = upload(&router, &raw).await;

    let result = detect(&router, &token, json!({})).await;
    assert_eq!(result["messages_detected"], json!(1));
    // Fields renamed for answering in parts (plan
    // 2026-10-06-load-only-what-the-page-shows.md §8, §10b): sessions done
    // of the total, and no cursor once the scan is complete.
    assert_eq!(result["sessions_total"], json!(1));
    assert_eq!(result["sessions_done"], json!(1));
    assert_eq!(result["cursor"], Value::Null);

    let text = export_text(&router, &token).await;
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    let auto = reparsed.conversations[0].chat_messages[0]
        .extra
        .get("_claude_timeline_auto")
        .expect("detected human message should carry embedded auto flags");
    // Specific expected values, not merely "a record exists" -- HUMAN_TEXT is
    // chosen to trip exactly these two.
    assert_eq!(
        auto["caps"],
        json!(true),
        "WRONG should trip the caps heuristic"
    );
    assert_eq!(
        auto["critical"],
        json!(true),
        "'you failed to' should trip the criticism heuristic"
    );
}

#[tokio::test]
async fn detection_never_gives_an_assistant_message_an_auto_flag_record() {
    let router = local_app::router();
    let raw = format!(
        "[{}]",
        one_conversation("11111111-1111-4111-8111-111111111111", "Hi")
    );
    let token = upload(&router, &raw).await;
    detect(&router, &token, json!({})).await;

    let text = export_text(&router, &token).await;
    let reparsed = unwrap_uploaded_json(&text).unwrap();
    let assistant = &reparsed.conversations[0].chat_messages[1];
    assert!(
        !assistant.extra.contains_key("_claude_timeline_auto"),
        "assistant messages should never carry auto flags"
    );
}

/// Rewritten for the time limit (plan
/// 2026-10-06-load-only-what-the-page-shows.md §8, §10b; it was
/// `paging_covers_every_conversation_exactly_once`): with a budget of one
/// step per request, each request reads one row and answers with where to
/// carry on; the parts cover every message exactly once, the sessions
/// done climb to the total, and the last part has no cursor.
#[tokio::test]
async fn the_scan_in_parts_covers_every_message_exactly_once() {
    let (router, _, _) = local_app::app_in_steps(1);
    let raw = format!(
        "[{},{},{}]",
        one_conversation("11111111-1111-4111-8111-111111111111", "One"),
        one_conversation("aaaaaaaa-1111-4111-8111-111111111111", "Two"),
        one_conversation("55555555-1111-4111-8111-111111111111", "Three"),
    );
    let token = upload(&router, &raw).await;

    let mut body = json!({});
    let mut parts = 0;
    let mut total_detected = 0;
    let mut done = Vec::new();
    loop {
        let result = detect(&router, &token, body).await;
        assert_eq!(result["sessions_total"], json!(3));
        total_detected += result["messages_detected"].as_u64().unwrap();
        done.push(result["sessions_done"].as_u64().unwrap());
        parts += 1;
        match result["cursor"].as_str() {
            Some(cursor) => body = json!({ "cursor": cursor }),
            None => break,
        }
        assert!(parts <= 10, "the scan did not finish");
    }
    // A step is one row read (plan §8b), Claude's replies included: two rows
    // per conversation, six parts.
    assert_eq!(parts, 6, "{done:?}");
    assert_eq!(total_detected, 3, "one human message per conversation");
    assert!(done.windows(2).all(|w| w[0] <= w[1]), "{done:?}");
    assert_eq!(done.last(), Some(&3));
}
