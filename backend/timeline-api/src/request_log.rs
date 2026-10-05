//! One log line per API request
//! (docs/plans/completed/2026-10-02-activity-instrumentation.md §3), in the
//! `api_request` format the timeline script reads (`scripts/activity_timeline.py`).
//!
//! [`with_request_log`] wraps a router so that every request it answers,
//! including refusals and unknown routes, runs inside
//! [`request_record::recording`] and then writes one JSON line: method,
//! route template (`/conversations/{conversation_id}/...`, never the raw
//! path), status, duration, user, the page's session id, API Gateway's
//! request id, the route's own facts and the AWS calls it made.
//!
//! The line goes to a [`LineSink`]: standard output on Lambda, which Lambda
//! ships to the function's log group; a collecting closure in tests. Writing
//! it is the last thing done, after the response is built, and nothing in it
//! can fail: the line is built from values that always serialize.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::Router;
use lambda_http::request::RequestContext;
use serde_json::{json, Value};

use crate::request_record::{self, RequestRecord};

/// The header carrying the page's session id (plan §1).
pub const SESSION_HEADER: &str = "x-timeline-session";

/// Where finished log lines go.
pub type LineSink = Arc<dyn Fn(&str) + Send + Sync>;

/// A sink that prints each line on standard output.
pub fn stdout_sink() -> LineSink {
    Arc::new(|line| println!("{line}"))
}

/// The page's session id: a UUID it makes once per page load. Parsed here,
/// at the edge, so a value that isn't one is never logged as if it were.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionId(pub uuid::Uuid);

impl SessionId {
    /// The session id in `headers`, if present and a UUID.
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Option<SessionId> {
        let value = headers.get(SESSION_HEADER)?.to_str().ok()?;
        uuid::Uuid::parse_str(value).ok().map(SessionId)
    }
}

/// The identifiers of the request being answered, for code that writes
/// further lines about it (`routes::activity`). Set by the request-log layer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestIds {
    /// API Gateway's id for the request; absent when not running on AWS.
    pub request_id: Option<String>,
    pub session: Option<SessionId>,
}

/// API Gateway's request id, from the context `lambda_http` attaches to each
/// request. Absent locally and in tests that don't attach one.
fn api_gateway_request_id(request: &Request) -> Option<String> {
    match request.extensions().get::<RequestContext>()? {
        RequestContext::ApiGatewayV2(context) => context.request_id.clone(),
        _ => None,
    }
}

/// Builds the `api_request` line. Public so its format is tested directly
/// against what the timeline script expects.
pub fn api_request_line(
    method: &str,
    route: Option<&str>,
    status: u16,
    millis: u128,
    ids: &RequestIds,
    record: &RequestRecord,
) -> String {
    json!({
        "kind": "api_request",
        "request_id": ids.request_id,
        "method": method,
        "route": route,
        "status": status,
        "ms": millis as u64,
        "user": record.user,
        "session": ids.session.map(|s| s.0.to_string()),
        "facts": Value::Object(record.facts.clone()),
        "aws_calls": record.aws_calls,
        "aws_retries": record.aws_retries,
        "aws_failures": record.aws_failures,
        "aws_errors": record.aws_errors,
    })
    .to_string()
}

async fn log_request(sink: LineSink, mut request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().to_string();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_string());
    let ids = RequestIds {
        request_id: api_gateway_request_id(&request),
        session: SessionId::from_headers(request.headers()),
    };
    request.extensions_mut().insert(ids.clone());
    let (response, record) = request_record::recording(next.run(request)).await;
    sink(&api_request_line(
        &method,
        route.as_deref(),
        response.status().as_u16(),
        started.elapsed().as_millis(),
        &ids,
        &record,
    ));
    response
}

/// Wraps `router` so every request it answers is logged to `sink`. Applied
/// after all routes are added: axum runs a router-wide layer once the route
/// is matched, which is what makes the route template available.
pub fn with_request_log(router: Router, sink: LineSink) -> Router {
    router.layer(middleware::from_fn(move |request: Request, next: Next| {
        log_request(sink.clone(), request, next)
    }))
}
