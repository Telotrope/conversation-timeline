//! `POST /activity` -- the page's record of what the user did, written to
//! the log one line per event
//! (docs/plans/2026-10-02-activity-instrumentation.md §5).
//!
//! On AWS this route runs in its own Lambda function (`bin/record_activity.rs`),
//! never in the API's, so a report arriving while one of the user's requests
//! is being answered can't make AWS start a second API copy for it.
//!
//! The page is not trusted to have checked anything: the batch size, each
//! event's size, its `kind` and the shape of its values are checked here,
//! and every string is cut to [`MAX_TEXT_CHARS`]. Anything else is refused
//! with 400 naming the problem. Nothing is stored except the log lines; the
//! log line is JSON, whose serializer escapes every character that means
//! something in JSON.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{FromRef, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use timeline_auth::cognito::CognitoVerifier;

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::request_log::{LineSink, RequestIds};
use crate::request_record::note;

pub const MAX_EVENTS: usize = 200;
pub const MAX_EVENT_BYTES: usize = 4096;
pub const MAX_TEXT_CHARS: usize = 200;

/// The kinds of event the page records (plan §4). Anything else is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEventKind {
    Click,
    Change,
    Submit,
    View,
    Shown,
    Request,
}

impl PageEventKind {
    pub fn parse(name: &str) -> Option<PageEventKind> {
        match name {
            "click" => Some(PageEventKind::Click),
            "change" => Some(PageEventKind::Change),
            "submit" => Some(PageEventKind::Submit),
            "view" => Some(PageEventKind::View),
            "shown" => Some(PageEventKind::Shown),
            "request" => Some(PageEventKind::Request),
            _ => None,
        }
    }
}

/// What this route needs: the login check, and where to write lines.
#[derive(Clone)]
pub struct ActivityState {
    pub verifier: Arc<CognitoVerifier>,
    pub sink: LineSink,
}

impl FromRef<ActivityState> for Arc<CognitoVerifier> {
    fn from_ref(state: &ActivityState) -> Self {
        state.verifier.clone()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityBatch {
    pub events: Vec<Value>,
    /// Events the page had to drop because it couldn't send them in time.
    #[serde(default)]
    pub dropped: u64,
}

fn capped(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

/// A value from the page, checked: strings are cut to length; objects are
/// allowed only as `nested == false` (one level deep); arrays never.
fn checked_value(value: &Value, nested: bool, at: &str) -> Result<Value, String> {
    match value {
        Value::String(s) => Ok(Value::String(capped(s))),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(value.clone()),
        Value::Object(fields) if !nested => {
            let mut out = Map::new();
            for (name, inner) in fields {
                out.insert(capped(name), checked_value(inner, true, at)?);
            }
            Ok(Value::Object(out))
        }
        Value::Object(_) => Err(format!("{at}: objects may be only one level deep")),
        Value::Array(_) => Err(format!("{at}: lists are not accepted")),
    }
}

/// One event, checked as the module doc describes; `number` counts from 1.
pub fn checked_event(event: &Value, number: usize) -> Result<Value, String> {
    let at = format!("event {number}");
    let size = event.to_string().len();
    if size > MAX_EVENT_BYTES {
        return Err(format!("{at} is {size} bytes; at most {MAX_EVENT_BYTES}"));
    }
    let Value::Object(fields) = event else {
        return Err(format!("{at} is not an object"));
    };
    let kind = fields
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{at} has no kind"))?;
    if PageEventKind::parse(kind).is_none() {
        return Err(format!("{at} has an unknown kind: {:?}", capped(kind)));
    }
    if !fields.get("t").is_some_and(Value::is_u64) {
        return Err(format!("{at} has no time (t, milliseconds)"));
    }
    let mut out = Map::new();
    for (name, value) in fields {
        out.insert(capped(name), checked_value(value, false, &at)?);
    }
    Ok(Value::Object(out))
}

/// The `page_event` line for one accepted event.
pub fn page_event_line(ids: &RequestIds, user: &str, event: &Value) -> String {
    json!({
        "kind": "page_event",
        "request_id": ids.request_id,
        "user": user,
        "session": ids.session.map(|s| s.0.to_string()),
        "event": event,
    })
    .to_string()
}

pub async fn record_activity(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sink): State<LineSink>,
    ids: Option<Extension<RequestIds>>,
    body: Result<Json<ActivityBatch>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(batch) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    if batch.events.len() > MAX_EVENTS {
        return Err(ApiError::BadRequest(format!(
            "{} events; at most {MAX_EVENTS} per batch",
            batch.events.len()
        )));
    }
    let events = batch
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| checked_event(event, index + 1))
        .collect::<Result<Vec<_>, _>>()
        .map_err(ApiError::BadRequest)?;
    let ids = ids.map(|Extension(ids)| ids).unwrap_or_default();
    let user = user_id.to_string();
    for event in &events {
        sink(&page_event_line(&ids, &user, event));
    }
    note("events", events.len());
    note("dropped", batch.dropped);
    Ok(StatusCode::NO_CONTENT)
}

impl FromRef<ActivityState> for LineSink {
    fn from_ref(state: &ActivityState) -> Self {
        state.sink.clone()
    }
}
