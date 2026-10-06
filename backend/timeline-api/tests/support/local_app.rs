//! The local app as tests drive it: the same stores and routers the local
//! server runs with ([`timeline_api::local_state`]), and helpers for the
//! steps most tests share: signing in, uploading a file through the local
//! upload route (which processes it, as S3's event does on AWS), and
//! reading a reply in parts to its end (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8c).

#![allow(dead_code)]

use std::num::NonZeroUsize;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use timeline_api::app::{build_dev_router, build_router};
use timeline_api::dev_state::DevState;
use timeline_api::flag_handles::FlagHandleKey;
use timeline_api::local_state::build_local_state;
use timeline_api::s3_trigger::ProcessingStores;
use timeline_api::state::AppState;
use timeline_core::conversation_metadata::UploadFacts;
use timeline_core::labels::{FileName, PersonName};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};
use tower::ServiceExt;

/// The local app with the clock budget AWS uses.
pub fn app() -> (Router, AppState, DevState) {
    app_with(BudgetSetting::Clock(REQUEST_WORK_LIMIT))
}

/// The local app with `steps` steps per request, so small data still
/// answers in several parts.
pub fn app_in_steps(steps: usize) -> (Router, AppState, DevState) {
    app_with(BudgetSetting::Steps(NonZeroUsize::new(steps).unwrap()))
}

pub fn app_with(budget: BudgetSetting) -> (Router, AppState, DevState) {
    let (app_state, dev_state) = build_local_state(FlagHandleKey::generate(), budget);
    let router = build_router(app_state.clone()).merge(build_dev_router(dev_state.clone()));
    (router, app_state, dev_state)
}

/// Just the router.
pub fn router() -> Router {
    app().0
}

/// The local app's processing stores, alone.
pub fn memory_stores() -> ProcessingStores {
    app().2.processing
}

pub async fn body_bytes(response: axum::response::Response) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

pub async fn body_json(response: axum::response::Response) -> Value {
    let bytes = body_bytes(response).await;
    serde_json::from_slice(&bytes)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&bytes)))
}

/// Sends `request`; the status and the body as JSON (`Null` when empty).
pub async fn send(router: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = body_bytes(response).await;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()))
    };
    (status, body)
}

/// A request to our API, signed in with `token`, with `body` as JSON.
pub fn request(method: &str, uri: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"));
    match body {
        Some(body) => builder
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub async fn get(router: &Router, token: &str, uri: &str) -> (StatusCode, Value) {
    send(router, request("GET", uri, token, None)).await
}

/// A dev login's token for `sub`.
pub async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    let (_, body) = send(router, request).await;
    body["token"].as_str().unwrap().to_string()
}

/// Uploads `raw` as `token`'s user through the real local flow (POST
/// /uploads, then PUT to the address it gives), which processes it; returns
/// the upload's id.
pub async fn upload(router: &Router, token: &str, raw: impl Into<Vec<u8>>) -> String {
    let (status, created) = send(
        router,
        request(
            "POST",
            "/uploads",
            token,
            Some(json!({"file_name": "conversations.json", "human_name": "Alice"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let put = Request::builder()
        .method("PUT")
        .uri(created["upload_url"].as_str().unwrap())
        .body(Body::from(raw.into()))
        .unwrap();
    let (status, body) = send(router, put).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    created["upload_id"].as_str().unwrap().to_string()
}

/// Signs in as `sub` and uploads `raw`; the token.
pub async fn signed_in_with(router: &Router, sub: &str, raw: &str) -> String {
    let token = dev_login(router, sub).await;
    upload(router, &token, raw.to_string()).await;
    token
}

/// `uri` with its query's `cursor` set.
fn with_cursor(uri: &str, cursor: Option<&str>) -> String {
    match cursor {
        None => uri.to_string(),
        Some(c) => {
            let joiner = if uri.contains('?') { '&' } else { '?' };
            format!("{uri}{joiner}cursor={c}")
        }
    }
}

/// Every part of a GET answered in parts, following the cursor until the
/// last part. Panics after 10,000 parts, so a cursor that never ends fails
/// the test rather than hanging it.
pub async fn all_parts(router: &Router, token: &str, uri: &str) -> Vec<Value> {
    let mut parts = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10_000 {
        let (status, part) = get(router, token, &with_cursor(uri, cursor.as_deref())).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {part}");
        cursor = part["cursor"].as_str().map(str::to_string);
        parts.push(part);
        if cursor.is_none() {
            return parts;
        }
    }
    panic!("{uri} never finished");
}

/// The items under `field` of every part, joined.
pub async fn all_of(router: &Router, token: &str, uri: &str, field: &str) -> Vec<Value> {
    all_parts(router, token, uri)
        .await
        .iter()
        .flat_map(|p| p[field].as_array().unwrap().clone())
        .collect()
}

/// Every conversation record of the user's (GET /conversations, in parts).
pub async fn conversations(router: &Router, token: &str) -> Vec<Value> {
    all_of(router, token, "/conversations", "conversations").await
}

/// Every session of the user's (GET /sessions, in parts).
pub async fn sessions(router: &Router, token: &str) -> Vec<Value> {
    all_of(router, token, "/sessions", "sessions").await
}

/// The annotated download's text, joined from its parts, and every flag
/// handle the parts carried.
pub async fn export(router: &Router, token: &str) -> (String, serde_json::Map<String, Value>) {
    let parts = all_parts(router, token, "/export").await;
    let mut text = String::new();
    let mut handles = serde_json::Map::new();
    for part in &parts {
        text.push_str(part["part"].as_str().unwrap());
        handles.extend(part["flag_handles"].as_object().unwrap().clone());
    }
    (text, handles)
}

/// The annotated download, parsed.
pub async fn exported(router: &Router, token: &str) -> Value {
    let (text, _) = export(router, token).await;
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("the download isn't JSON ({e}): {text}"))
}

/// Every row of Review for `query` (GET /messages, with the rest of the
/// query), following the cursor and asking each part for the rows still
/// wanted, 50 at most per part as the page does. Returns the rows in order.
pub async fn review_rows(router: &Router, token: &str, query: &str) -> Vec<Value> {
    let mut rows = Vec::new();
    let mut cursor: Option<String> = None;
    let mut matched = 0;
    let mut notes = 0;
    for _ in 0..10_000 {
        let mut uri = format!("/messages?rows=50&until=rows&matched={matched}&notes={notes}");
        if !query.is_empty() {
            uri.push('&');
            uri.push_str(query);
        }
        let (status, part) = get(router, token, &with_cursor(&uri, cursor.as_deref())).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {part}");
        rows.extend(part["rows"].as_array().unwrap().clone());
        matched = part["matched"].as_u64().unwrap();
        notes = part["notes"].as_u64().unwrap();
        cursor = part["cursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            return rows;
        }
    }
    panic!("/messages never finished");
}

/// Runs the scan to its end; the number of parts it took.
pub async fn scan(router: &Router, token: &str) -> usize {
    let mut cursor: Option<String> = None;
    for part in 1..10_000 {
        let body = match &cursor {
            None => json!({}),
            Some(c) => json!({ "cursor": c }),
        };
        let (status, reply) = send(router, request("POST", "/detect", token, Some(body))).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        cursor = reply["cursor"].as_str().map(str::to_string);
        if cursor.is_none() {
            return part;
        }
    }
    panic!("the scan never finished");
}

/// Records what `POST /uploads` would have, for tests that put an upload's
/// bytes in place themselves.
pub async fn record_upload_facts(stores: &ProcessingStores, user_id: &UserId, upload_id: UploadId) {
    stores
        .upload_outcome_store
        .record_received(
            user_id,
            upload_id,
            UploadFacts {
                file_name: FileName::parse("conversations.json").unwrap(),
                uploaded_at: chrono::DateTime::from_timestamp(1_760_000_000, 0).unwrap(),
                file_written_at: None,
                human_name: PersonName::parse("Alice").unwrap(),
            },
        )
        .await
        .unwrap();
}
