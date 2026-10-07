//! Requests that answer in parts carry on correctly from their cursors (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8c's table of
//! resumption tests), for every request that answers in parts. The local
//! server is started with a budget of 1, 2 or 3 steps per request, so this
//! small data answers in many parts, and the joined parts are compared with
//! the answer one request gives under the clock.

#[path = "support/exports.rs"]
mod exports;
#[path = "support/local_app.rs"]
mod local_app;

use axum::http::StatusCode;
use axum::Router;
use exports::{at, claude, conv, conversation, export, msg, you};
use serde_json::{json, Value};
use timeline_api::cursor::Cursor;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::stored_message::EntryKey;
use timeline_core::stored_session::SessionKey;
use timeline_core::walk_cursor::WalkCursor;

/// A (Zebra): sessions at minutes 0-7 and 60-61; B (Apple): 3-5, inside
/// A's first session in time; C (Mango): the next day.
fn data() -> Vec<String> {
    let mut a = conversation(
        1,
        "Zebra",
        &[
            you(0, "first in Zebra"),
            claude(1, "reply one"),
            you(6, "This is WRONG"),
            claude(7, "reply two"),
            you(60, "back later"),
            claude(61, "welcome back"),
        ],
    );
    a["chat_messages"][2]["_claude_timeline_user"] = json!({"critical": true});
    let b = conversation(
        2,
        "Apple",
        &[
            you(3, "an apple question"),
            claude(4, "apple answer"),
            you(5, "thanks, you FAILED"),
        ],
    );
    let c = conversation(
        3,
        "Mango",
        &[you(1440, "the next day"), claude(1441, "mango reply")],
    );
    // Three files, so the file list has parts too.
    vec![export(vec![a]), export(vec![b]), export(vec![c])]
}

/// A router with `steps` steps per request (or the clock, for `None`),
/// alice's three files uploaded, and her token.
async fn world(steps: Option<usize>) -> (Router, String) {
    let router = match steps {
        Some(n) => local_app::app_in_steps(n).0,
        None => local_app::router(),
    };
    let token = local_app::dev_login(&router, "alice").await;
    for file in data() {
        local_app::upload(&router, &token, file).await;
    }
    (router, token)
}

/// What the read-only requests answer, joined from their parts, ignoring
/// fields that only say how far a part got.
async fn everything(router: &Router, token: &str) -> Value {
    let mut uploads = local_app::all_of(router, token, "/uploads", "uploads").await;
    uploads.sort_by(|a, b| a["upload_id"].as_str().cmp(&b["upload_id"].as_str()));
    let (download, _) = local_app::export(router, token).await;
    json!({
        "conversations": local_app::conversations(router, token).await,
        "sessions": local_app::sessions(router, token).await,
        "uploads": uploads,
        "review": local_app::review_rows(router, token, "").await,
        "review_flagged": local_app::review_rows(router, token, "flag=flagged&replies=true").await,
        "review_day": local_app::review_rows(router, token, &format!(
            "from={}&to={}&span=day", "2026-03-02T00%3A00%3A00Z", "2026-03-02T23%3A59%3A59Z")).await,
        "files": local_app::all_of(router, token, &format!("/conversations/{}/files", conv(1)), "files").await,
        "download": download,
    })
}

/// Parts add up to the whole: with 1, 2 and 3 steps per request, every
/// read-only request, joined from its parts, equals the one-request answer.
#[tokio::test]
async fn parts_add_up_to_the_one_request_answer() {
    let (router, token) = world(None).await;
    let whole = everything(&router, &token).await;
    assert_eq!(whole["review"].as_array().unwrap().len(), 6);
    for steps in [1, 2, 3] {
        let (router, token) = world(Some(steps)).await;
        let parts = everything(&router, &token).await;
        for field in whole.as_object().unwrap().keys() {
            let without_handles = |v: &Value| {
                strip(
                    v,
                    &["handle", "uploaded_at", "upload_id", "source", "additions"],
                )
            };
            assert_eq!(
                without_handles(&parts[field]),
                without_handles(&whole[field]),
                "{field} with {steps} steps"
            );
        }
    }
}

/// `value` without the named fields anywhere in it: handles are signed with
/// each server's own key, and upload times and ids differ between the two
/// servers' uploads.
fn strip(value: &Value, names: &[&str]) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !names.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), strip(v, names)))
                .collect(),
        ),
        Value::Array(list) => Value::Array(list.iter().map(|v| strip(v, names)).collect()),
        Value::String(s) if s.starts_with("{\"claude_timeline_format_version\"") => {
            strip(&serde_json::from_str(s).unwrap(), names)
        }
        other => other.clone(),
    }
}

/// The scan in parts writes the same flags as in one request, and scans
/// every message of yours exactly once.
#[tokio::test]
async fn the_scan_in_parts_writes_every_flag_exactly_once() {
    let (router, token) = world(None).await;
    assert_eq!(local_app::scan(&router, &token).await, 1);
    let whole = local_app::review_rows(&router, &token, "").await;
    for steps in [1, 2, 3] {
        let (router, token) = world(Some(steps)).await;
        let mut cursor: Option<String> = None;
        let mut detected = 0;
        let mut parts = 0;
        loop {
            let body = cursor
                .as_ref()
                .map_or(json!({}), |c| json!({ "cursor": c }));
            let (status, reply) = local_app::send(
                &router,
                local_app::request("POST", "/detect", &token, Some(body)),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{reply}");
            detected += reply["messages_detected"].as_u64().unwrap();
            parts += 1;
            cursor = reply["cursor"].as_str().map(str::to_string);
            if cursor.is_none() {
                assert_eq!(reply["sessions_done"], reply["sessions_total"]);
                break;
            }
        }
        assert_eq!(
            detected, 6,
            "each of your six messages once, with {steps} steps"
        );
        assert!(parts > 1, "{steps} steps: {parts} parts");
        let rows = local_app::review_rows(&router, &token, "").await;
        assert_eq!(
            strip(&json!(rows), &["handle"]),
            strip(&json!(whole), &["handle"]),
            "{steps} steps"
        );
    }
}

/// The two server analyses, carried on from their saved rows request after
/// request, end with the same numbers as one request.
#[tokio::test]
async fn the_server_analyses_in_parts_end_with_the_same_numbers() {
    let (router, token) = world(None).await;
    for uri in [
        "/analyses/trend?granularity=week&tz=America%2FNew_York",
        "/analyses/time-of-day?view=yours&tz=UTC",
    ] {
        let (_, whole) = local_app::get(&router, &token, uri).await;
        assert_eq!(whole["status"], "done", "{whole}");
        for steps in [1, 2, 3] {
            let (router, token) = world(Some(steps)).await;
            let mut working = 0;
            let done = loop {
                let (status, reply) = local_app::get(&router, &token, uri).await;
                assert_eq!(status, StatusCode::OK, "{reply}");
                if reply["status"] == "done" {
                    break reply;
                }
                working += 1;
                assert!(working < 1000, "never finished");
            };
            assert!(working > 0, "{steps} steps should take several requests");
            assert_eq!(
                done["numbers"], whole["numbers"],
                "{uri} with {steps} steps"
            );
        }
    }
}

/// Editing a file's details in parts changes every conversation of the
/// file once; repeating the edit is harmless.
#[tokio::test]
async fn a_file_edit_in_parts_changes_each_conversation_once() {
    let router = local_app::app_in_steps(1).0;
    let token = local_app::dev_login(&router, "alice").await;
    let both = export(vec![
        conversation(1, "One", &[you(0, "a")]),
        conversation(2, "Two", &[you(5, "b")]),
    ]);
    let upload = local_app::upload(&router, &token, both).await;
    let medium = json!({"kind": "live_voice", "transcription": {"service": "otter_ai"}});
    let mut body = json!({ "medium": medium });
    let mut changed = Vec::new();
    let mut done = Vec::new();
    loop {
        let (status, part) = local_app::send(
            &router,
            local_app::request(
                "PUT",
                &format!("/uploads/{upload}/metadata"),
                &token,
                Some(body.clone()),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{part}");
        assert_eq!(part["total"], 2);
        done.push(part["done"].as_u64().unwrap());
        changed.extend(
            part["conversations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["conversation_id"].clone()),
        );
        match part["cursor"].as_str() {
            Some(c) => body["cursor"] = json!(c),
            None => break,
        }
    }
    assert_eq!(changed, vec![json!(conv(1)), json!(conv(2))]);
    assert_eq!(done, vec![1, 2]);
    for record in local_app::conversations(&router, &token).await {
        assert_eq!(record["medium"], medium);
    }
    // A part answered after the last conversation is done says so.
    body["cursor"] = json!(Cursor::UploadEdit {
        after: ConversationId(conv(2).parse().unwrap())
    }
    .to_text());
    let (_, after_all) = local_app::send(
        &router,
        local_app::request(
            "PUT",
            &format!("/uploads/{upload}/metadata"),
            &token,
            Some(body),
        ),
    )
    .await;
    assert_eq!(after_all["done"], 2);
    assert_eq!(after_all["cursor"], Value::Null);
}

/// The cursors of a one-step walk stop anywhere: inside a session, at a
/// session's last row and at a conversation's end; the last part has none.
#[tokio::test]
async fn a_cursor_can_stop_anywhere() {
    let (router, token) = world(Some(1)).await;
    let mut cursor: Option<String> = None;
    let mut stops: Vec<WalkCursor> = Vec::new();
    loop {
        let uri = match &cursor {
            None => "/detect".to_string(),
            Some(_) => "/detect".to_string(),
        };
        let body = cursor
            .as_ref()
            .map_or(json!({}), |c| json!({ "cursor": c }));
        let (_, reply) = local_app::send(
            &router,
            local_app::request("POST", &uri, &token, Some(body)),
        )
        .await;
        cursor = reply["cursor"].as_str().map(str::to_string);
        match &cursor {
            Some(text) => match Cursor::parse(text).unwrap() {
                Cursor::Scan { walk } => stops.push(walk),
                other => panic!("the scan's cursor is a scan cursor, not {other:?}"),
            },
            None => break,
        }
    }
    let a = ConversationId(conv(1).parse().unwrap());
    // Mid-session: after A's first message, its reply still to read.
    assert!(
        stops.contains(&WalkCursor {
            group: SessionKey {
                conversation_id: a,
                number: 0
            },
            after: Some(key(1, 0, 1)),
        }),
        "{stops:?}"
    );
    // At a session's last row: once A's first session's last row (the
    // reply at 7) is done, the cursor names the next session, nothing done
    // in it yet.
    assert!(
        stops.contains(&WalkCursor {
            group: session_key(1, 1),
            after: None
        }),
        "{stops:?}"
    );
    // At a conversation's end: after A's last row (61), B's session.
    assert!(
        stops.contains(&WalkCursor {
            group: session_key(2, 0),
            after: None
        }),
        "{stops:?}"
    );
    // Every stop names a session and a row of it, or a session's start.
    assert!(stops.iter().all(|s| s
        .after
        .is_none_or(|k| k.conversation_id == s.group.conversation_id)));
}

/// The key of message `n` of `conversation`, numbered from 1 in its file,
/// so its position is `n - 1` (plan §12.3).
fn key(conversation: u32, minute: i64, n: u32) -> EntryKey {
    EntryKey {
        conversation_id: ConversationId(conv(conversation).parse().unwrap()),
        position: timeline_core::stored_message::Position(i64::from(n) - 1),
        at: chrono::DateTime::parse_from_rfc3339(&at(minute))
            .unwrap()
            .with_timezone(&chrono::Utc),
        id: MessageId(msg(conversation, n).parse().unwrap()),
    }
}

fn session_key(conversation: u32, number: usize) -> SessionKey {
    SessionKey {
        conversation_id: ConversationId(conv(conversation).parse().unwrap()),
        number,
    }
}

/// Starting from a cursor built for a known point in the data (the user's
/// suggestion), with no time limit to run out, returns exactly the answer
/// worked out in advance for the rest of the data.
#[tokio::test]
async fn starting_from_a_given_cursor_returns_the_rest_worked_out_in_advance() {
    let (router, token) = world(None).await;
    let rest = |walk: WalkCursor| {
        let (router, token) = (router.clone(), token.clone());
        async move {
            let cursor = Cursor::Messages { walk }.to_text();
            let (status, part) =
                local_app::get(&router, &token, &format!("/messages?cursor={cursor}")).await;
            assert_eq!(status, StatusCode::OK, "{part}");
            part["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["message_id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    // Review's groups in time order: A's first session with B's (they
    // overlap), then A's second, then C's.
    // Mid-group, after B's first message at minute 3: B's thanks (5), A's
    // WRONG (6), A's back later (60), C (1440).
    assert_eq!(
        rest(WalkCursor {
            group: session_key(1, 0),
            after: Some(key(2, 3, 1))
        })
        .await,
        vec![msg(2, 3), msg(1, 3), msg(1, 5), msg(3, 1)]
    );
    // At the start of a group: A's second session, then C.
    assert_eq!(
        rest(WalkCursor {
            group: session_key(1, 1),
            after: None
        })
        .await,
        vec![msg(1, 5), msg(3, 1)]
    );
    // After a conversation's end (A's last row at 61): C.
    assert_eq!(
        rest(WalkCursor {
            group: session_key(1, 1),
            after: Some(key(1, 61, 6))
        })
        .await,
        vec![msg(3, 1)]
    );
    // After the very last row: nothing, and no cursor.
    let cursor = Cursor::Messages {
        walk: WalkCursor {
            group: session_key(3, 0),
            after: Some(key(3, 1441, 2)),
        },
    }
    .to_text();
    let (_, part) = local_app::get(&router, &token, &format!("/messages?cursor={cursor}")).await;
    assert_eq!(part["rows"], json!([]));
    assert_eq!(part["cursor"], Value::Null);
    assert_eq!(part["sessions_done"], part["sessions_total"]);

    // The scan from a known point (key order: each session alone) scans
    // exactly the rest: B's second message of yours, then C's.
    let scan_from = Cursor::Scan {
        walk: WalkCursor {
            group: session_key(2, 0),
            after: Some(key(2, 4, 2)),
        },
    }
    .to_text();
    let (_, reply) = local_app::send(
        &router,
        local_app::request(
            "POST",
            "/detect",
            &token,
            Some(json!({ "cursor": scan_from })),
        ),
    )
    .await;
    assert_eq!(reply["messages_detected"], 2, "{reply}");
    assert_eq!(reply["cursor"], Value::Null);
    let flagged = local_app::review_rows(&router, &token, "flag=flagged&view=automatic").await;
    assert_eq!(
        flagged
            .iter()
            .map(|r| r["message_id"].clone())
            .collect::<Vec<_>>(),
        vec![json!(msg(2, 3))],
        "only the rest was scanned; FAILED is flagged, the next day's isn't"
    );
}

/// A search answered in parts finds each match once, none skipped.
#[tokio::test]
async fn a_search_in_parts_finds_each_match_exactly_once() {
    for steps in [1, 2, 3] {
        let (router, token) = world(Some(steps)).await;
        let rows = local_app::review_rows(&router, &token, "search=E").await;
        let ids: Vec<&str> = rows
            .iter()
            .map(|r| r["message_id"].as_str().unwrap())
            .collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            ids.len(),
            unique.len(),
            "no match twice with {steps} steps: {ids:?}"
        );
        // Every message of yours but "This is WRONG" holds an e.
        assert_eq!(ids.len(), 5, "{ids:?}");
    }
}

/// A flag saved between two parts changes the data version the next part
/// reports, which tells the page to start that request over.
#[tokio::test]
async fn a_change_between_parts_changes_the_data_version() {
    let (router, token) = world(Some(1)).await;
    let (_, first) = local_app::get(&router, &token, "/messages").await;
    let row = &first["rows"][0];
    let (status, saved) = local_app::send(
        &router,
        local_app::request(
            "PATCH",
            &format!(
                "/conversations/{}/messages/{}/flags",
                row["conversation_id"].as_str().unwrap(),
                row["message_id"].as_str().unwrap()
            ),
            &token,
            Some(json!({"angry": true, "handle": row["handle"]})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_ne!(saved["data_version"], first["data_version"]);
    let next = format!("/messages?cursor={}", first["cursor"].as_str().unwrap());
    let (_, second) = local_app::get(&router, &token, &next).await;
    assert_eq!(second["data_version"], saved["data_version"]);
    assert_ne!(second["data_version"], first["data_version"]);
}

/// A bad cursor is refused with a 400 naming why: text that isn't a cursor,
/// one far too long, another request's, or one naming a session that no
/// longer starts a part of the results.
#[tokio::test]
async fn a_bad_cursor_is_refused_naming_why() {
    let (router, token) = world(None).await;
    let scan_cursor = Cursor::Scan {
        walk: WalkCursor {
            group: session_key(1, 0),
            after: None,
        },
    }
    .to_text();
    let gone = Cursor::Messages {
        walk: WalkCursor {
            group: session_key(9, 0),
            after: None,
        },
    }
    .to_text();
    let not_a_group_start = Cursor::Messages {
        walk: WalkCursor {
            group: session_key(2, 0),
            after: None,
        },
    }
    .to_text();
    let not_json = base64_of("not json");
    let too_long = "A".repeat(3000);
    let cases = [
        ("/messages?cursor=%25%25", "not the expected encoding"),
        (
            &*format!("/messages?cursor={not_json}"),
            "not the expected content",
        ),
        (&*format!("/messages?cursor={too_long}"), "far longer"),
        (
            &*format!("/messages?cursor={scan_cursor}"),
            "another kind of request, not GET /messages",
        ),
        (&*format!("/messages?cursor={gone}"), "no longer starts"),
        (
            &*format!("/messages?cursor={not_a_group_start}"),
            "no longer starts",
        ),
        (
            &*format!("/sessions?cursor={scan_cursor}"),
            "not GET /sessions",
        ),
        (
            &*format!("/conversations?cursor={scan_cursor}"),
            "not GET /conversations",
        ),
        (
            &*format!("/uploads?cursor={scan_cursor}"),
            "not GET /uploads",
        ),
        (&*format!("/export?cursor={scan_cursor}"), "not GET /export"),
        (
            &*format!("/conversations/{}/files?cursor={scan_cursor}", conv(1)),
            "files",
        ),
    ];
    for (uri, mentions) in cases {
        let (status, body) = local_app::get(&router, &token, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert!(
            body["error"].as_str().unwrap().contains(mentions),
            "{uri}: {body}"
        );
    }
    let messages_cursor = Cursor::Messages {
        walk: WalkCursor {
            group: session_key(1, 0),
            after: None,
        },
    }
    .to_text();
    for (method, uri, body) in [
        (
            "POST",
            "/detect".to_string(),
            json!({ "cursor": messages_cursor }),
        ),
        (
            "PUT",
            format!("/uploads/{}/metadata", uuid::Uuid::nil()),
            json!({ "medium": {"kind": "typed"}, "cursor": messages_cursor }),
        ),
    ] {
        let (status, reply) = local_app::send(
            &router,
            local_app::request(method, &uri, &token, Some(body)),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {reply}");
        assert!(
            reply["error"].as_str().unwrap().contains("another kind"),
            "{uri}: {reply}"
        );
    }
}

fn base64_of(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text)
}

/// A cursor made from alice's request, sent by bob, reads only bob's rows:
/// the user always comes from the sign-in, never from the cursor.
#[tokio::test]
async fn another_users_data_cant_be_reached_with_a_cursor() {
    let (router, alice) = world(Some(1)).await;
    let bob = local_app::dev_login(&router, "bob").await;
    // Bob has the same first file (same ids), with no review in it.
    let mut file = data().remove(0);
    file = file.replace(",\"_claude_timeline_user\":{\"critical\":true}", "");
    local_app::upload(&router, &bob, file).await;
    let (_, part) = local_app::get(&router, &alice, "/messages?flag=overridden").await;
    let cursor = part["cursor"].as_str().unwrap().to_string();
    let (status, bobs) = local_app::get(
        &router,
        &bob,
        &format!("/messages?flag=overridden&cursor={cursor}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{bobs}");
    assert_eq!(bobs["rows"], json!([]), "bob reviewed nothing");
    let (status, _) = local_app::get(&router, &bob, &format!("/messages?cursor={cursor}")).await;
    assert_eq!(status, StatusCode::OK);
    // A cursor into a conversation bob doesn't have names no part of his.
    let alices_c = Cursor::Messages {
        walk: WalkCursor {
            group: session_key(3, 0),
            after: None,
        },
    }
    .to_text();
    let (status, body) =
        local_app::get(&router, &bob, &format!("/messages?cursor={alices_c}")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}
