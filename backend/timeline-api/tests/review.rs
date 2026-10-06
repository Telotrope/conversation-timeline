//! `GET /messages`: Review's rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b, §8c),
//! through the real routes on the local server's stores: every filter, the
//! order (by time across conversations; for a Calendar day by conversation
//! name), notes where branches were pruned, Claude's replies, page-start
//! cursors and the two ways a walk stops.

#[path = "support/exports.rs"]
mod exports;
#[path = "support/local_app.rs"]
mod local_app;

use axum::http::StatusCode;
use axum::Router;
use exports::{at, claude, conv, conversation, export, msg, you};
use serde_json::{json, Value};

/// Two conversations whose sessions overlap in time, and a third a day
/// later: A (Zebra) at minutes 0-6, B (Apple) at 3-5, C (Mango) at 1440.
fn three_conversations() -> String {
    let mut a = conversation(
        1,
        "Zebra",
        &[
            you(0, "first in Zebra"),
            claude(1, "reply one"),
            you(6, "This is WRONG"),
            claude(7, "reply two"),
        ],
    );
    // Your review of A's second message: not caps, critical.
    a["chat_messages"][2]["_claude_timeline_user"] = json!({"caps": false, "critical": true});
    let b = conversation(
        2,
        "Apple",
        &[
            you(3, "an apple question"),
            claude(4, "apple answer"),
            you(5, "thanks"),
        ],
    );
    let c = conversation(
        3,
        "Mango",
        &[you(1440, "the next day"), claude(1441, "mango reply")],
    );
    export(vec![a, b, c])
}

async fn setup() -> (Router, String) {
    let router = local_app::router();
    let token = local_app::signed_in_with(&router, "alice", &three_conversations()).await;
    (router, token)
}

fn ids(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|r| r["message_id"].as_str().unwrap_or("note").to_string())
        .collect()
}

#[tokio::test]
async fn your_messages_come_in_time_order_across_conversations() {
    let (router, token) = setup().await;
    let rows = local_app::review_rows(&router, &token, "").await;
    assert_eq!(
        ids(&rows),
        vec![msg(1, 1), msg(2, 1), msg(2, 3), msg(1, 3), msg(3, 1)],
        "time order, interleaving the overlapping conversations"
    );
    let first = &rows[0];
    assert_eq!(first["kind"], "message");
    assert_eq!(first["conversation_id"], conv(1));
    assert_eq!(first["at"], at(0).replace("+00:00", "Z"));
    assert_eq!(first["pieces"][0]["text"], "first in Zebra");
    assert_eq!(
        first["flags"],
        json!({"auto": null, "user": {"caps": null, "critical": null, "angry": null}})
    );
    assert_eq!(first["handle"].as_str().unwrap().len(), 43);
    assert_eq!(first["reply"], Value::Null, "replies only when asked for");
}

#[tokio::test]
async fn a_conversation_a_span_and_a_day_each_narrow_the_rows() {
    let (router, token) = setup().await;
    let rows = local_app::review_rows(&router, &token, &format!("conversation={}", conv(2))).await;
    assert_eq!(ids(&rows), vec![msg(2, 1), msg(2, 3)]);
    // A session's span (minutes 3-5 of B, given as from/to), ends included.
    let span = format!("from={}&to={}", urlencode(&at(3)), urlencode(&at(5)));
    let rows = local_app::review_rows(&router, &token, &span).await;
    assert_eq!(ids(&rows), vec![msg(2, 1), msg(2, 3)]);
    let both = format!("{span}&conversation={}", conv(1));
    assert_eq!(
        ids(&local_app::review_rows(&router, &token, &both).await),
        Vec::<String>::new()
    );
}

/// A Calendar day (`span=day`) lists its rows grouped by conversation name,
/// in time within each, as the page always has.
#[tokio::test]
async fn a_calendar_day_groups_rows_by_conversation_name() {
    let (router, token) = setup().await;
    let day = format!(
        "from={}&to={}&span=day",
        urlencode("2026-03-02T00:00:00Z"),
        urlencode("2026-03-02T23:59:59.999Z")
    );
    let rows = local_app::review_rows(&router, &token, &day).await;
    assert_eq!(
        ids(&rows),
        vec![msg(2, 1), msg(2, 3), msg(1, 1), msg(1, 3)],
        "Apple before Zebra; Mango is the next day"
    );
}

#[tokio::test]
async fn the_flag_menu_and_the_view_choose_rows() {
    let (router, token) = setup().await;
    let q = |query: &str| {
        let (router, token) = (router.clone(), token.clone());
        let query = query.to_string();
        async move { ids(&local_app::review_rows(&router, &token, &query).await) }
    };
    assert_eq!(q("flag=overridden").await, vec![msg(1, 3)]);
    assert_eq!(q("flag=critical").await, vec![msg(1, 3)]);
    assert_eq!(q("flag=flagged&view=yours").await, vec![msg(1, 3)]);
    // Unscanned, so nothing automatic; your critical flag isn't shown
    // with only automatic flags.
    assert_eq!(q("flag=flagged&view=automatic").await, Vec::<String>::new());
    assert_eq!(q("flag=caps").await, Vec::<String>::new());
    // After the scan, the automatic caps flag shows; your "not caps" wins
    // with both shown.
    local_app::scan(&router, &token).await;
    assert_eq!(q("flag=caps&view=automatic").await, vec![msg(1, 3)]);
    assert_eq!(q("flag=caps&view=both").await, Vec::<String>::new());
    assert_eq!(q("flag=angry").await, Vec::<String>::new());
    assert_eq!(
        q("flag=all&view=neither").await.len(),
        5,
        "every message, unflagged"
    );
}

#[tokio::test]
async fn search_matches_letters_inside_words_ignoring_capitals() {
    let (router, token) = setup().await;
    let found = local_app::review_rows(&router, &token, "search=%20APPLE%20").await;
    assert_eq!(ids(&found), vec![msg(2, 1)]);
    let inside = local_app::review_rows(&router, &token, "search=ron").await;
    assert_eq!(ids(&inside), vec![msg(1, 3)], "WRONG holds ron");
    let blank = local_app::review_rows(&router, &token, "search=%20%20").await;
    assert_eq!(blank.len(), 5, "a blank search is no search");
}

#[tokio::test]
async fn replies_come_with_each_row_when_asked_for() {
    let (router, token) = setup().await;
    let rows = local_app::review_rows(&router, &token, "replies=true").await;
    let replies: Vec<Value> = rows
        .iter()
        .map(|r| r["reply"]["pieces"][0]["text"].clone())
        .collect();
    assert_eq!(
        replies,
        vec![
            json!("reply one"),
            json!("apple answer"),
            Value::Null,
            json!("reply two"),
            json!("mango reply")
        ],
        "B's last message has no reply"
    );
    assert_eq!(rows[0]["reply"]["message_id"], msg(1, 2));
}

/// A replaced branch's note is listed in its place with every message, but
/// not under a flag filter or a search: notes match neither.
#[tokio::test]
async fn notes_are_listed_in_place_only_without_a_flag_filter_or_search() {
    let router = local_app::router();
    let mut c = conversation(
        1,
        "Branchy",
        &[you(0, "question"), claude(1, "answer"), you(5, "thanks")],
    );
    // A resend of the question at minute 2, answering the start: replaced.
    c["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 9), "parent_message_uuid": exports::ROOT, "sender": "human",
        "created_at": at(2), "content": [{"type": "text", "text": "question"}],
    }));
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let rows = local_app::review_rows(&router, &token, "").await;
    assert_eq!(ids(&rows), vec![msg(1, 1), "note".to_string(), msg(1, 3)]);
    let note = &rows[1];
    assert_eq!(note["kind"], "note");
    assert_eq!(note["messages"], 1);
    assert_eq!(note["words_not_repeated"], 0);
    assert_eq!(note["replaced_by"], msg(1, 1));
    assert_eq!(note["kept_as"], Value::Null);
    let (_, part) = local_app::get(&router, &token, "/messages").await;
    assert_eq!(
        (part["matched"].clone(), part["notes"].clone()),
        (json!(3), json!(1))
    );
    let searched = local_app::review_rows(&router, &token, "search=question").await;
    assert_eq!(ids(&searched), vec![msg(1, 1)]);
    let flagged = local_app::review_rows(&router, &token, "flag=overridden").await;
    assert!(flagged.is_empty());
}

/// More than a page of rows: the first part gives page 0's 50 rows and the
/// start of every later page it reaches; asking from page 1's start with
/// `until=rows` gives exactly page 1, and stops.
#[tokio::test]
async fn page_starts_let_the_page_ask_for_any_page() {
    let router = local_app::router();
    // Distinct texts: identical ones in a row would be removed as resends.
    let texts: Vec<String> = (0..120).map(|i| format!("message {i}")).collect();
    let messages: Vec<exports::M> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| you(i as i64 * 20, t))
        .collect();
    let token = local_app::signed_in_with(
        &router,
        "alice",
        &export(vec![conversation(1, "Long", &messages)]),
    )
    .await;
    let (status, first) = local_app::get(&router, &token, "/messages").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(first["rows"].as_array().unwrap().len(), 50);
    assert_eq!(first["matched"], 120);
    assert_eq!(
        first["cursor"],
        Value::Null,
        "counted to the end in one part"
    );
    let starts = first["page_starts"].as_array().unwrap();
    assert_eq!(
        starts
            .iter()
            .map(|s| s["page"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let page_1 = format!(
        "/messages?rows=50&until=rows&matched=50&cursor={}",
        starts[0]["cursor"].as_str().unwrap()
    );
    let (_, second) = local_app::get(&router, &token, &page_1).await;
    let rows = second["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 50);
    assert_eq!(rows[0]["message_id"], msg(1, 51));
    assert_eq!(rows[49]["message_id"], msg(1, 100));
    assert_eq!(second["matched"], 100, "stopped once its rows were full");
    assert!(second["cursor"].is_string());
    let page_2 = format!(
        "/messages?rows=50&until=rows&matched=100&cursor={}",
        starts[1]["cursor"].as_str().unwrap()
    );
    let (_, third) = local_app::get(&router, &token, &page_2).await;
    assert_eq!(third["rows"].as_array().unwrap().len(), 20);
    assert_eq!(third["matched"], 120);
    assert_eq!(third["cursor"], Value::Null);
    // Asking for no rows only counts; more than 50 are never sent.
    let (_, count) = local_app::get(&router, &token, "/messages?rows=0").await;
    assert_eq!(count["rows"], json!([]));
    assert_eq!(count["matched"], 120);
    let (_, capped) = local_app::get(&router, &token, "/messages?rows=500").await;
    assert_eq!(capped["rows"].as_array().unwrap().len(), 50);
}

#[tokio::test]
async fn a_bad_query_is_a_bad_request_naming_it() {
    let (router, token) = setup().await;
    for (query, mentions) in [
        ("flag=shouting", "flag must be one of"),
        ("from=2026-01-01T00:00:00Z", "needs both from and to"),
        (
            "from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z",
            "can't end before it starts",
        ),
        ("view=sideways", "view"),
        ("colour=red", "colour"),
        ("cursor=not-a-cursor", "cursor refused"),
    ] {
        let (status, body) = local_app::get(&router, &token, &format!("/messages?{query}")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
        assert!(
            body["error"].as_str().unwrap().contains(mentions),
            "{query}: {body}"
        );
    }
}

/// Messages of unknown time (§4e): found by their conversation and through
/// their session's span, never by a Calendar day.
#[tokio::test]
async fn messages_of_unknown_time_are_found_through_their_session_not_a_day() {
    let router = local_app::router();
    let mut c = conversation(1, "Undated", &[you(0, "timed"), claude(1, "reply")]);
    c["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 9), "sender": "human", "content": [{"type": "text", "text": "when?"}],
    }));
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let by_conversation =
        local_app::review_rows(&router, &token, &format!("conversation={}", conv(1))).await;
    assert_eq!(by_conversation.len(), 2);
    let untimed = by_conversation
        .iter()
        .find(|r| r["message_id"] == msg(1, 9))
        .unwrap();
    assert_eq!(untimed["at"], Value::Null);
    // The conversation is one session placed by its record's span (its
    // known times here); a span overlapping it finds the untimed message.
    let sessions = local_app::sessions(&router, &token).await;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["placement"], "span");
    let span = format!(
        "from={}&to={}",
        urlencode(sessions[0]["start"].as_str().unwrap()),
        urlencode(sessions[0]["end"].as_str().unwrap())
    );
    assert_eq!(
        local_app::review_rows(&router, &token, &span).await.len(),
        2
    );
    let day = format!(
        "from={}&to={}&span=day",
        urlencode("2026-03-02T00:00:00Z"),
        urlencode("2026-03-02T23:59:59Z")
    );
    let rows = local_app::review_rows(&router, &token, &day).await;
    assert_eq!(
        ids(&rows),
        vec![msg(1, 1)],
        "a day never finds the untimed one"
    );
}

fn urlencode(text: &str) -> String {
    text.replace('+', "%2B").replace(':', "%3A")
}
