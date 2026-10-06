//! The routes added or changed by plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` (§5, §5c, §6,
//! §8b, §8c), beyond what `parts.rs` and `review.rs` cover: the server
//! analyses, a flag save's recount, totals and the data version in the
//! replies, the upload status's progress, and the test-only step budget.

#[path = "support/exports.rs"]
mod exports;
#[path = "support/local_app.rs"]
mod local_app;

use axum::http::StatusCode;
use axum::Router;
use exports::{claude, conv, conversation, export, msg, you};
use serde_json::{json, Value};
use timeline_api::local_state::{budget_from, InvalidBudget, BUDGET_STEPS_VAR};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::ProcessingProgress;
use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};

async fn with_data() -> (Router, String) {
    let router = local_app::router();
    let raw = export(vec![
        conversation(
            1,
            "One",
            &[you(0, "This is WRONG"), claude(1, "sorry"), you(2, "fine")],
        ),
        conversation(2, "Two", &[you(600, "hello")]),
    ]);
    let token = local_app::signed_in_with(&router, "alice", &raw).await;
    (router, token)
}

async fn save(router: &Router, token: &str, row: &Value, body: Value) -> (StatusCode, Value) {
    let mut body = body;
    body["handle"] = row["handle"].clone();
    local_app::send(
        router,
        local_app::request(
            "PATCH",
            &format!(
                "/conversations/{}/messages/{}/flags",
                row["conversation_id"].as_str().unwrap(),
                row["message_id"].as_str().unwrap()
            ),
            token,
            Some(body),
        ),
    )
    .await
}

/// A flag save recounts its message's session in the same request (§6),
/// answers with it, and raises the data version (§5c).
#[tokio::test]
async fn a_flag_save_recounts_its_session_and_raises_the_version() {
    let (router, token) = with_data().await;
    let rows = local_app::review_rows(&router, &token, "").await;
    let (_, before) = local_app::get(&router, &token, "/sessions").await;
    let (status, saved) = save(&router, &token, &rows[0], json!({"critical": true})).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["scanned"], false);
    assert_eq!(saved["session"]["counts"]["reviewed"], 1);
    assert_eq!(saved["session"]["counts"]["yours"]["critical"], 1);
    assert_eq!(saved["session"]["counts"]["both"]["any"], 1);
    assert_eq!(
        saved["data_version"].as_u64(),
        before["data_version"].as_u64().map(|v| v + 1)
    );
    let sessions = local_app::sessions(&router, &token).await;
    assert_eq!(
        sessions[0], saved["session"],
        "the stored session is the one answered"
    );
    assert_eq!(sessions[0]["counts"]["messages"], 2);
}

/// Claude's messages and messages that aren't stored can't be saved: they
/// are not found, with a real handle for the ids named.
#[tokio::test]
async fn a_save_on_claudes_message_or_one_not_stored_is_not_found() {
    let (router, state, _) = local_app::app();
    let raw = export(vec![conversation(1, "One", &[you(0, "a"), claude(1, "b")])]);
    let token = local_app::signed_in_with(&router, "alice", &raw).await;
    let alice = UserId("alice".to_string());
    for message in [msg(1, 2), msg(1, 77)] {
        let handle = state.flag_handle_key.handle_for(
            &alice,
            timeline_core::model::ConversationId(conv(1).parse().unwrap()),
            timeline_core::model::MessageId(message.parse().unwrap()),
        );
        let row = json!({"conversation_id": conv(1), "message_id": message, "handle": handle});
        let (status, body) = save(&router, &token, &row, json!({"caps": true})).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{message}: {body}");
        let (status, _) = local_app::get(
            &router,
            &token,
            &format!("/conversations/{}/messages/{message}/flags", conv(1)),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}

/// Every reply in parts carries the totals and the data version (§8b).
#[tokio::test]
async fn totals_and_the_data_version_come_with_every_part() {
    let (router, token) = with_data().await;
    let (_, conversations) = local_app::get(&router, &token, "/conversations").await;
    assert_eq!(conversations["total"], 2);
    assert_eq!(conversations["data_version"], 1);
    let (_, sessions) = local_app::get(&router, &token, "/sessions").await;
    assert_eq!(sessions["total"], 2);
    let (_, uploads) = local_app::get(&router, &token, "/uploads").await;
    assert_eq!(uploads["total"], 1);
    let (_, messages) = local_app::get(&router, &token, "/messages").await;
    assert_eq!(
        (
            messages["sessions_done"].clone(),
            messages["sessions_total"].clone()
        ),
        (json!(2), json!(2))
    );
}

/// The scan raises the data version when it scanned something, not when
/// there was nothing to scan.
#[tokio::test]
async fn the_scan_raises_the_version_only_when_it_scans() {
    let router = local_app::router();
    let token = local_app::dev_login(&router, "alice").await;
    let (_, empty) = local_app::send(
        &router,
        local_app::request("POST", "/detect", &token, Some(json!({}))),
    )
    .await;
    assert_eq!(empty["data_version"], 0);
    assert_eq!(empty["sessions_total"], 0);
    let (status, bad) = local_app::send(
        &router,
        local_app::request("POST", "/detect", &token, Some(json!({"offset": 0}))),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
}

#[tokio::test]
async fn the_server_analyses_count_your_messages_in_your_zone() {
    let (router, token) = with_data().await;
    let (status, trend) = local_app::get(&router, &token, "/analyses/trend?tz=UTC").await;
    assert_eq!(status, StatusCode::OK, "{trend}");
    assert_eq!(trend["status"], "done");
    assert_eq!(trend["numbers"]["kind"], "trend");
    assert_eq!(trend["numbers"]["granularity"], "week");
    assert_eq!(
        trend["numbers"]["buckets"],
        json!({"2026-W09": {"total": 3, "flagged": 0}})
    );
    let (_, month) =
        local_app::get(&router, &token, "/analyses/trend?granularity=month&tz=UTC").await;
    assert_eq!(
        month["numbers"]["buckets"],
        json!({"2026-03": {"total": 3, "flagged": 0}})
    );
    let (_, hours) =
        local_app::get(&router, &token, "/analyses/time-of-day?tz=Asia%2FKolkata").await;
    assert_eq!(hours["numbers"]["kind"], "time_of_day");
    // 09:00 UTC is 14:30 in Kolkata; minute 600 is 19:00 UTC, 00:30 there.
    assert_eq!(
        hours["numbers"]["by_hour"][14],
        json!({"total": 2, "flagged": 0})
    );
    assert_eq!(
        hours["numbers"]["by_hour"][0],
        json!({"total": 1, "flagged": 0})
    );
    // With only your flags shown, only reviewed messages count.
    let (_, yours) = local_app::get(&router, &token, "/analyses/trend?view=yours&tz=UTC").await;
    assert_eq!(yours["numbers"]["buckets"], json!({}));
}

/// A saved result is used while the data version is the same; a change
/// (here the scan) makes the next request count afresh.
#[tokio::test]
async fn a_saved_analysis_is_counted_again_after_the_data_changes() {
    let (router, token) = with_data().await;
    let uri = "/analyses/trend?tz=UTC&view=automatic";
    let (_, before) = local_app::get(&router, &token, uri).await;
    assert_eq!(before["numbers"]["buckets"]["2026-W09"]["flagged"], 0);
    let (_, again) = local_app::get(&router, &token, uri).await;
    assert_eq!(again, before, "the saved result, unchanged");
    local_app::scan(&router, &token).await;
    let (_, after) = local_app::get(&router, &token, uri).await;
    assert_eq!(
        after["numbers"]["buckets"]["2026-W09"]["flagged"], 1,
        "WRONG"
    );
    assert_ne!(after["data_version"], before["data_version"]);
}

#[tokio::test]
async fn an_unknown_analysis_or_zone_is_refused() {
    let (router, token) = with_data().await;
    let (status, _) = local_app::get(&router, &token, "/analyses/friction?tz=UTC").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = local_app::get(&router, &token, "/analyses/trend?tz=Mars%2FOlympus").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("time zone"),
        "{body}"
    );
    let (status, _) = local_app::get(&router, &token, "/analyses/trend").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "tz is required");
    let (status, _) =
        local_app::get(&router, &token, "/analyses/trend?tz=UTC&granularity=day").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The upload status carries how far processing has got (§8b).
#[tokio::test]
async fn the_upload_status_carries_processing_progress() {
    let (router, _, dev) = local_app::app();
    let token = local_app::dev_login(&router, "alice").await;
    let upload = UploadId(uuid::Uuid::from_u128(5));
    let progress = ProcessingProgress {
        bytes_read: 10,
        bytes_total: 40,
        conversations_written: 0,
        conversations_total: 0,
    };
    dev.processing
        .upload_outcome_store
        .record_processing_progress(&UserId("alice".to_string()), upload, progress)
        .await
        .unwrap();
    let (status, body) = local_app::get(&router, &token, &format!("/uploads/{upload}")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"status": "processing", "progress": {
            "bytes_read": 10, "bytes_total": 40, "conversations_written": 0, "conversations_total": 0
        }})
    );
}

/// The local test server's step budget: a whole number of 1 or more, or
/// the clock when unset.
#[test]
fn the_step_budget_setting_is_a_whole_number_or_the_clock() {
    assert_eq!(
        budget_from(None),
        Ok(BudgetSetting::Clock(REQUEST_WORK_LIMIT))
    );
    assert_eq!(
        budget_from(Some(" 3 ")),
        Ok(BudgetSetting::Steps(
            std::num::NonZeroUsize::new(3).unwrap()
        ))
    );
    for bad in ["0", "-1", "lots", ""] {
        let err = budget_from(Some(bad)).unwrap_err();
        assert_eq!(err, InvalidBudget(bad.to_string()));
        assert!(err.to_string().contains(BUDGET_STEPS_VAR), "{err}");
    }
}

/// More records and sessions than storage hands over in one page (100 and
/// 200) are all read, in parts or not.
#[tokio::test]
async fn more_than_one_storage_page_of_records_and_sessions_is_read() {
    let router = local_app::router();
    // 130 conversations, two sessions each: 260 sessions.
    let texts: Vec<(String, String)> = (0..130)
        .map(|i| (format!("a{i}"), format!("b{i}")))
        .collect();
    let conversations: Vec<Value> = texts
        .iter()
        .enumerate()
        .map(|(i, (a, b))| conversation(i as u32 + 1, "c", &[you(0, a), you(60, b)]))
        .collect();
    let token = local_app::signed_in_with(&router, "alice", &export(conversations)).await;
    assert_eq!(local_app::conversations(&router, &token).await.len(), 130);
    assert_eq!(local_app::sessions(&router, &token).await.len(), 260);
}

/// An edit with nothing in it is refused, as before.
#[tokio::test]
async fn a_conversation_edit_with_nothing_to_save_is_refused() {
    let (router, token) = with_data().await;
    let (status, body) = local_app::send(
        &router,
        local_app::request(
            "PUT",
            &format!("/conversations/{}/metadata", conv(1)),
            &token,
            Some(json!({})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap().contains("nothing to save"),
        "{body}"
    );
}

/// Review stops as soon as its rows are full (`until=rows`), even in the
/// middle of a group of sessions, and carries on from there.
#[tokio::test]
async fn review_stops_with_its_rows_full_inside_a_session() {
    let router = local_app::router();
    let texts: Vec<String> = (0..60).map(|i| format!("message {i}")).collect();
    let messages: Vec<exports::M> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| you(i as i64, t))
        .collect();
    let token = local_app::signed_in_with(
        &router,
        "alice",
        &export(vec![conversation(1, "One", &messages)]),
    )
    .await;
    let (_, first) = local_app::get(&router, &token, "/messages?rows=50&until=rows").await;
    assert_eq!(first["rows"].as_array().unwrap().len(), 50);
    assert_eq!(first["matched"], 50);
    let next = format!(
        "/messages?rows=50&until=rows&matched=50&cursor={}",
        first["cursor"].as_str().unwrap()
    );
    let (_, second) = local_app::get(&router, &token, &next).await;
    assert_eq!(second["rows"].as_array().unwrap().len(), 10);
    assert_eq!(second["rows"][0]["message_id"], msg(1, 51));
    assert_eq!(second["cursor"], Value::Null);
}
