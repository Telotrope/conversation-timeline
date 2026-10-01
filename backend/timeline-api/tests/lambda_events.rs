//! Requests in the format API Gateway hands Lambda, converted by
//! `lambda_http` exactly as the deployed function converts them, then
//! answered by the API Lambda's own router against the local stand-ins.
//! See the migration plan's §V2e, E1.
//!
//! Why this exists: `lambda_http` 0.15 puts a named stage in front of the
//! path (`/dev/conversations` for stage `dev`), and the router only knows
//! `/conversations`, so every request on a named stage would have been a
//! 404. The template now uses the `$default` stage, which adds nothing.
//!
//! The events start from a sample shipped with `aws_lambda_events`, not one
//! captured from this project's deployment; see
//! `tests/fixtures/aws-samples/README.md` and the plan's C24.
//!
//! Needs Java and DynamoDB Local, like timeline-storage's DynamoDB tests;
//! fails (never skips) without them.

#[path = "support/aws_world.rs"]
#[allow(dead_code)]
mod aws_world;
#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "../../timeline-storage/tests/support/s3_local.rs"]
#[allow(dead_code)]
mod s3_local;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};

use aws_world::{call, World};

const SAMPLE: &str =
    include_str!("fixtures/aws-samples/example-apigw-v2-request-jwt-authorizer.json");
const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The sample event, changed to a `GET` of `path` on `stage` carrying
/// `token`. Only the fields the conversion reads for routing and the
/// header the router reads for login are changed.
fn event(path: &str, stage: &str, token: &str) -> String {
    let mut event: Value = serde_json::from_str(SAMPLE).unwrap();
    event["rawPath"] = json!(path);
    event["rawQueryString"] = json!("");
    event["queryStringParameters"] = json!({});
    event["body"] = Value::Null;
    event["requestContext"]["stage"] = json!(stage);
    event["requestContext"]["http"]["method"] = json!("GET");
    event["requestContext"]["http"]["path"] = json!(path);
    event["headers"] = json!({ "authorization": format!("Bearer {token}") });
    event.to_string()
}

/// Converts an event the way `lambda_http::run` does before handing it to
/// the router.
fn convert(event: &str) -> Request<Body> {
    let request = lambda_http::request::from_str(event).expect("a valid HTTP API event");
    let (parts, body) = request.into_parts();
    Request::from_parts(parts, Body::from(body.to_vec()))
}

#[tokio::test]
async fn a_request_on_the_default_stage_reaches_its_route() {
    let world = World::new().await;
    let token = world.pool_token("alice");
    let (status, body) = call(
        &world.lambda_router(),
        convert(&event("/conversations", "$default", &token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(serde_json::from_slice::<Value>(&body).unwrap(), json!([]));
}

/// Pins the reason for using `$default`: on a named stage the path the
/// router sees gains the stage's name, and nothing answers it.
#[tokio::test]
async fn a_request_on_a_named_stage_is_not_found() {
    let world = World::new().await;
    let token = world.pool_token("alice");
    let (status, _) = call(
        &world.lambda_router(),
        convert(&event("/conversations", "dev", &token)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The template must keep the stage that adds nothing to the path. A plain
/// text check, crude in the way the plan's C16 describes (CloudFormation's
/// `!Sub` tags stop ordinary YAML readers), but it fails if the line goes.
#[test]
fn the_template_uses_the_default_stage() {
    let api = TEMPLATE
        .split("\n  HttpApi:\n")
        .nth(1)
        .expect("template declares HttpApi");
    let api = api.split("\n  # ---").next().unwrap();
    assert!(
        api.contains("StageName: \"$default\""),
        "HttpApi must use the $default stage:\n{api}"
    );
}
