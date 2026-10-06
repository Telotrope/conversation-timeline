//! One `api_request` log line per request, through real requests to the
//! router wrapped by `with_request_log`
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3). Each test reads
//! the lines a collecting sink received, in the format the timeline script
//! (`scripts/activity_timeline.py`) reads.
//!
//! Proves, through the public router: the route template is logged, never
//! the raw path (axum's `MatchedPath`); the user comes from the login check
//! (`auth_extractor` records it); the page's session header is logged only
//! when it is a UUID (`request_log::SessionId`); API Gateway's request id is
//! taken from the context `lambda_http` attaches; and each route's facts
//! (`request_record::note`) reach the line.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use lambda_http::aws_lambda_events::apigw::ApiGatewayV2httpRequestContext;
use lambda_http::request::RequestContext;
use serde_json::{json, Value};
use timeline_api::request_log::{
    api_request_line, with_request_log, LineSink, RequestIds, SessionId,
};
use timeline_api::request_record::RequestRecord;
use tower::ServiceExt;

#[path = "support/local_app.rs"]
mod local_app;

const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");
const SESSION: &str = "5c1e2a3b-4d5e-4f60-8172-839405a6b7c8";

/// The local router, logged to a sink whose lines the test can read.
fn logged_router() -> (Router, Arc<Mutex<Vec<String>>>) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let collected = lines.clone();
    let sink: LineSink = Arc::new(move |line| collected.lock().unwrap().push(line.to_string()));
    (with_request_log(local_app::router(), sink), lines)
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn request(method: &str, uri: &str, token: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-timeline-session", SESSION);
    if let Some(token) = token {
        builder = builder.header("Authorization", format!("Bearer {token}"));
    }
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

/// Logs in, uploads the fixture and returns the token and export reply.
async fn signed_in_with_upload(router: &Router, sub: &str) -> (String, Value) {
    let (_, login) = send(
        router,
        request("POST", "/_dev/login", None, Some(json!({ "sub": sub }))),
    )
    .await;
    let token = login["token"].as_str().unwrap().to_string();
    let (_, created) = send(
        router,
        request(
            "POST",
            "/uploads",
            Some(&token),
            Some(json!({ "file_name": "conversations.json", "human_name": "Alice" })),
        ),
    )
    .await;
    let put = Request::builder()
        .method("PUT")
        .uri(created["upload_url"].as_str().unwrap())
        .body(Body::from(FIXTURE))
        .unwrap();
    router.clone().oneshot(put).await.unwrap();
    let (_, export) = send(router, request("GET", "/export", Some(&token), None)).await;
    (token, export)
}

fn lines_for(lines: &Arc<Mutex<Vec<String>>>, method: &str, route: &str) -> Vec<Value> {
    lines
        .lock()
        .unwrap()
        .iter()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|l| l["method"] == method && l["route"] == route)
        .collect()
}

#[tokio::test]
async fn every_request_gets_one_line_with_route_status_user_and_session() {
    let (router, lines) = logged_router();
    let (token, _) = signed_in_with_upload(&router, "alice").await;
    lines.lock().unwrap().clear();

    let (status, _) = send(
        &router,
        request("GET", "/conversations", Some(&token), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let all = lines.lock().unwrap().clone();
    assert_eq!(all.len(), 1, "{all:?}");
    let line: Value = serde_json::from_str(&all[0]).unwrap();
    assert_eq!(line["kind"], "api_request");
    assert_eq!(line["method"], "GET");
    assert_eq!(line["route"], "/conversations");
    assert_eq!(line["status"], 200);
    assert_eq!(line["user"], "alice");
    assert_eq!(line["session"], SESSION);
    assert_eq!(line["request_id"], Value::Null, "no API Gateway locally");
    assert!(line["ms"].is_u64());
    assert_eq!(
        line["aws_calls"],
        json!({}),
        "no AWS SDK in the local stores"
    );
    assert_eq!(line["aws_retries"], 0);
}

#[tokio::test]
async fn a_flag_save_logs_the_route_template_and_what_was_saved() {
    let (router, lines) = logged_router();
    let (token, export) = signed_in_with_upload(&router, "alice").await;
    let parsed: Value = serde_json::from_str(FIXTURE).unwrap();
    let (conversation, message) = parsed
        .as_array()
        .unwrap()
        .iter()
        .find_map(|c| {
            let human = c["chat_messages"]
                .as_array()?
                .iter()
                .find(|m| m["sender"] == "human")?;
            Some((c, human))
        })
        .expect("the fixture has a message from the human");
    let (conversation_id, message_id) = (
        conversation["uuid"].as_str().unwrap(),
        message["uuid"].as_str().unwrap(),
    );
    let handle = export["flag_handles"][message_id].clone();
    let uri = format!("/conversations/{conversation_id}/messages/{message_id}/flags");

    let saved = send(
        &router,
        request(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "handle": handle, "caps": true })),
        ),
    )
    .await;
    assert_eq!(saved.0, StatusCode::OK);
    let refused = send(
        &router,
        request(
            "PATCH",
            &uri,
            Some(&token),
            Some(json!({ "handle": "not-a-handle", "angry": false })),
        ),
    )
    .await;
    assert_eq!(refused.0, StatusCode::FORBIDDEN);

    let template = "/conversations/{conversation_id}/messages/{message_id}/flags";
    let logged = lines_for(&lines, "PATCH", template);
    assert_eq!(
        logged.len(),
        2,
        "the raw path is never the route: {logged:?}"
    );
    assert_eq!(logged[0]["status"], 200);
    assert_eq!(
        logged[0]["facts"],
        json!({
            "conversation_id": conversation_id,
            "message_id": message_id,
            "caps": true,
            "critical": null,
            "angry": null,
            "handle": "accepted",
        })
    );
    assert_eq!(logged[1]["status"], 403);
    assert_eq!(logged[1]["facts"]["handle"], "refused");
    assert_eq!(logged[1]["facts"]["angry"], false);
}

#[tokio::test]
async fn detection_uploads_and_exports_log_their_facts() {
    let (router, lines) = logged_router();
    let (token, _) = signed_in_with_upload(&router, "alice").await;

    let (status, page) = send(
        &router,
        request("POST", "/detect", Some(&token), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The scan's facts changed with the scan (plan
    // 2026-10-06-load-only-what-the-page-shows.md §8, §10b): sessions done
    // of the total and the messages scanned, and whether it carried on from
    // a cursor.
    let detect = &lines_for(&lines, "POST", "/detect")[0];
    assert_eq!(
        detect["facts"],
        json!({
            "resumed": false,
            "sessions_done": page["sessions_done"],
            "sessions_total": page["sessions_total"],
            "messages_detected": page["messages_detected"],
        })
    );
    assert_eq!(page["sessions_done"], page["sessions_total"]);
    let upload = &lines_for(&lines, "POST", "/uploads")[0];
    assert!(uuid::Uuid::parse_str(upload["facts"]["upload_id"].as_str().unwrap()).is_ok());
    let export = &lines_for(&lines, "GET", "/export")[0];
    assert_eq!(export["facts"]["conversations"], 6);
    assert!(export["facts"]["export_bytes"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn refused_and_unknown_requests_are_logged_too() {
    let (router, lines) = logged_router();

    let (no_login, _) = send(&router, request("GET", "/conversations", None, None)).await;
    assert_eq!(no_login, StatusCode::UNAUTHORIZED);
    let (unknown, _) = send(&router, request("GET", "/no-such-route", None, None)).await;
    assert_eq!(unknown, StatusCode::NOT_FOUND);

    let all: Vec<Value> = lines
        .lock()
        .unwrap()
        .iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0]["route"], "/conversations");
    assert_eq!(all[0]["status"], 401);
    assert_eq!(all[0]["user"], Value::Null);
    assert_eq!(
        all[1]["route"],
        Value::Null,
        "an unknown route has no template"
    );
    assert_eq!(all[1]["status"], 404);
}

#[tokio::test]
async fn a_session_header_that_is_not_a_uuid_is_not_logged_as_one() {
    let (router, lines) = logged_router();
    let bad = Request::builder()
        .uri("/conversations")
        .header("x-timeline-session", "\"; injected")
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(bad).await.unwrap();
    let missing = Request::builder()
        .uri("/conversations")
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(missing).await.unwrap();

    for line in lines.lock().unwrap().iter() {
        let line: Value = serde_json::from_str(line).unwrap();
        assert_eq!(line["session"], Value::Null, "{line}");
    }
}

#[tokio::test]
async fn api_gateways_request_id_is_logged_when_lambda_http_attaches_it() {
    let (router, lines) = logged_router();
    let mut context = ApiGatewayV2httpRequestContext::default();
    context.request_id = Some("EoYfxgfgIAMEb_A=".to_string());
    let mut on_aws = request("GET", "/conversations", None, None);
    on_aws
        .extensions_mut()
        .insert(RequestContext::ApiGatewayV2(context));
    router.clone().oneshot(on_aws).await.unwrap();

    let line: Value = serde_json::from_str(&lines.lock().unwrap()[0]).unwrap();
    assert_eq!(line["request_id"], "EoYfxgfgIAMEb_A=");

    // A REST-style API Gateway context isn't what this API is behind; its
    // id is not taken.
    let mut rest = request("GET", "/conversations", None, None);
    rest.extensions_mut()
        .insert(RequestContext::ApiGatewayV1(Default::default()));
    router.clone().oneshot(rest).await.unwrap();
    let line: Value = serde_json::from_str(&lines.lock().unwrap()[1]).unwrap();
    assert_eq!(line["request_id"], Value::Null);
}

#[test]
fn the_lambda_sink_prints_without_failing() {
    timeline_api::request_log::stdout_sink()("{\"kind\":\"api_request\"}");
}

#[test]
fn the_line_carries_every_recorded_count() {
    let mut record = RequestRecord::default();
    record.user = Some("alice".to_string());
    record.aws_calls.insert("DynamoDB.PutItem".to_string(), 410);
    record.aws_calls.insert("S3.GetObject".to_string(), 1);
    record.aws_retries = 2;
    record
        .aws_failures
        .insert("DynamoDB.PutItem".to_string(), 1);
    record
        .aws_errors
        .push("DynamoDB.PutItem: service error".to_string());
    let ids = RequestIds {
        request_id: Some("abc".to_string()),
        session: Some(SessionId(uuid::Uuid::parse_str(SESSION).unwrap())),
    };
    let line: Value = serde_json::from_str(&api_request_line(
        "POST",
        Some("/detect"),
        200,
        3210,
        &ids,
        &record,
    ))
    .unwrap();
    assert_eq!(
        line,
        json!({
            "kind": "api_request",
            "request_id": "abc",
            "method": "POST",
            "route": "/detect",
            "status": 200,
            "ms": 3210,
            "user": "alice",
            "session": SESSION,
            "facts": {},
            "aws_calls": { "DynamoDB.PutItem": 410, "S3.GetObject": 1 },
            "aws_retries": 2,
            "aws_failures": { "DynamoDB.PutItem": 1 },
            "aws_errors": ["DynamoDB.PutItem: service error"],
        })
    );
}
