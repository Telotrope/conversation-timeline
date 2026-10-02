//! `POST /activity`, through real requests to the activity router wrapped by
//! the request log, as `bin/record_activity.rs` runs it
//! (docs/plans/2026-10-02-activity-instrumentation.md §5). Every check the
//! route makes on the page's batch is shown refusing with 400 and naming
//! the problem; an accepted batch becomes one `page_event` line per event,
//! carrying the request's ids and the signed-in user.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use timeline_api::app::build_activity_router;
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::request_log::{with_request_log, LineSink};
use timeline_api::routes::activity::{ActivityState, PageEventKind, MAX_EVENTS, MAX_TEXT_CHARS};
use timeline_api::routes::dev_login::{login, DevLoginRequest};
use timeline_auth::cognito::CognitoVerifier;
use tower::ServiceExt;

const SESSION: &str = "5c1e2a3b-4d5e-4f60-8172-839405a6b7c8";

fn activity_router() -> (Router, Arc<Mutex<Vec<String>>>) {
    let (_, jwks) = &*DEV_KEYPAIR;
    let lines = Arc::new(Mutex::new(Vec::new()));
    let collected = lines.clone();
    let sink: LineSink = Arc::new(move |line| collected.lock().unwrap().push(line.to_string()));
    let state = ActivityState {
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
        sink: sink.clone(),
    };
    (with_request_log(build_activity_router(state), sink), lines)
}

/// A token for `sub`, from the local login route, signed with the dev key
/// pair the router trusts.
async fn token(sub: &str) -> String {
    let axum::Json(reply) = login(axum::Json(DevLoginRequest {
        sub: sub.to_string(),
    }))
    .await
    .expect("dev login");
    reply.token
}

async fn post(router: &Router, body: &str, signed_in: bool) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/activity")
        .header("content-type", "application/json")
        .header("x-timeline-session", SESSION);
    if signed_in {
        builder = builder.header("Authorization", format!("Bearer {}", token("alice").await));
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

fn parsed(lines: &Arc<Mutex<Vec<String>>>) -> Vec<Value> {
    lines
        .lock()
        .unwrap()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn click(t: u64) -> Value {
    json!({ "kind": "click", "t": t, "tab": "review",
            "target": { "tag": "button", "id": "loadBtn", "label": "Load" } })
}

#[tokio::test]
async fn a_valid_batch_becomes_one_line_per_event_then_the_request_line() {
    let (router, lines) = activity_router();
    let body = json!({ "events": [click(1), { "kind": "view", "t": 2, "tab": "review", "view": "review", "via": "hashchange" }], "dropped": 3 });

    let (status, _) = post(&router, &body.to_string(), true).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let all = parsed(&lines);
    assert_eq!(all.len(), 3, "{all:?}");
    for (line, event) in all[..2].iter().zip(body["events"].as_array().unwrap()) {
        assert_eq!(line["kind"], "page_event");
        assert_eq!(line["user"], "alice");
        assert_eq!(line["session"], SESSION);
        assert_eq!(line["request_id"], Value::Null);
        assert_eq!(&line["event"], event);
    }
    assert_eq!(all[2]["kind"], "api_request");
    assert_eq!(all[2]["route"], "/activity");
    assert_eq!(all[2]["status"], 204);
    assert_eq!(all[2]["facts"], json!({ "events": 2, "dropped": 3 }));
}

#[tokio::test]
async fn every_kind_the_page_sends_is_accepted() {
    let (router, _) = activity_router();
    let events: Vec<Value> = ["click", "change", "submit", "view", "shown", "request"]
        .iter()
        .map(|kind| {
            assert!(PageEventKind::parse(kind).is_some());
            json!({ "kind": kind, "t": 1 })
        })
        .collect();
    let (status, body) = post(&router, &json!({ "events": events }).to_string(), true).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn strings_are_cut_to_length() {
    let (router, lines) = activity_router();
    let long = "x".repeat(MAX_TEXT_CHARS + 50);
    let event = json!({ "kind": "submit", "t": 1, "text": long, "target": { "label": long } });
    post(&router, &json!({ "events": [event] }).to_string(), true).await;

    let line = &parsed(&lines)[0];
    assert_eq!(
        line["event"]["text"].as_str().unwrap().chars().count(),
        MAX_TEXT_CHARS
    );
    assert_eq!(
        line["event"]["target"]["label"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        MAX_TEXT_CHARS
    );
}

#[tokio::test]
async fn bad_batches_are_refused_naming_the_problem_and_log_no_events() {
    let too_many: Vec<Value> = (0..=MAX_EVENTS as u64).map(click).collect();
    let huge = json!({ "kind": "click", "t": 1, "a": "y".repeat(150), "b": "y".repeat(150),
        "c": { "d": "y".repeat(150) } });
    let mut oversized = huge.clone();
    for n in 0..40 {
        oversized[format!("field{n}")] = json!("z".repeat(150));
    }
    let cases: Vec<(String, &str)> = vec![
        (json!({ "events": too_many }).to_string(), "at most 200 per batch"),
        (json!({ "events": [oversized] }).to_string(), "at most 4096"),
        (json!({ "events": [{ "kind": "hover", "t": 1 }] }).to_string(), "unknown kind: \"hover\""),
        (json!({ "events": [{ "t": 1 }] }).to_string(), "has no kind"),
        (json!({ "events": [{ "kind": "click" }] }).to_string(), "has no time"),
        (json!({ "events": [{ "kind": "click", "t": -5 }] }).to_string(), "has no time"),
        (json!({ "events": ["click"] }).to_string(), "is not an object"),
        (json!({ "events": [{ "kind": "click", "t": 1, "target": { "inner": { "deep": 1 } } }] }).to_string(), "one level deep"),
        (json!({ "events": [{ "kind": "click", "t": 1, "list": [1, 2] }] }).to_string(), "lists are not accepted"),
        (json!({ "events": [], "extra": true }).to_string(), "unknown field"),
        ("not json".to_string(), ""),
    ];
    for (body, expected) in cases {
        let (router, lines) = activity_router();
        let (status, reply) = post(&router, &body, true).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {reply}");
        // The error is JSON, {"error": "..."}, except axum's own refusal of
        // a body that isn't JSON at all, which is plain text.
        let message = serde_json::from_str::<Value>(&reply)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or(reply.clone());
        assert!(message.contains(expected), "{expected:?} not in {message}");
        let logged = parsed(&lines);
        assert_eq!(logged.len(), 1, "only the request line: {logged:?}");
        assert_eq!(logged[0]["kind"], "api_request");
    }
}

#[tokio::test]
async fn a_report_without_a_sign_in_is_refused() {
    let (router, lines) = activity_router();
    let (status, _) = post(&router, &json!({ "events": [click(1)] }).to_string(), false).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let logged = parsed(&lines);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0]["status"], 401);
}
