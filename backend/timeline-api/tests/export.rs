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

#[path = "support/local_app.rs"]
mod local_app;

use axum::http::StatusCode;
use serde_json::json;
use timeline_core::unwrap_uploaded_json;

#[tokio::test]
async fn export_embeds_the_auto_flags_that_detection_computed() {
    let router = local_app::router();
    let raw = r#"[{"uuid":"11111111-1111-4111-8111-111111111111","name":"Hi","chat_messages":[
        {"uuid":"22222222-2222-4222-8222-222222222222","sender":"human","created_at":"2024-01-01T00:00:00Z","content":[{"type":"text","text":"WRONG, you failed to fix it."}]},
        {"uuid":"33333333-3333-4333-8333-333333333333","sender":"assistant","created_at":"2024-01-01T00:01:00Z","content":[{"type":"text","text":"Sorry, let me retry."}]}
    ]}]"#;
    let token = local_app::signed_in_with(&router, "alice", raw).await;

    // Flags only exist once they have been asked for.
    let (status, _) = local_app::send(
        &router,
        local_app::request("POST", "/detect", &token, Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The download now comes back in the replies themselves, in parts (plan
    // 2026-10-06-load-only-what-the-page-shows.md §8c), not from a stored
    // file's address; the last part has no cursor.
    let parts = local_app::all_parts(&router, &token, "/export").await;
    assert_eq!(parts.last().unwrap()["cursor"], serde_json::Value::Null);
    let text: String = parts.iter().map(|p| p["part"].as_str().unwrap()).collect();

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
