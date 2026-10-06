//! The Lambda's own router, built by `build_aws_state` exactly as `main.rs`
//! builds it under Lambda, run against local stand-ins: S3 served by
//! `s3s-fs`, DynamoDB by DynamoDB Local, and Cognito's published keys by a
//! small local HTTP server. See the migration plan's §V2d.
//!
//! Not covered: the real Lambda runtime, real Cognito and real AWS. Those
//! wait for the first deployment.
//!
//! Needs Java and DynamoDB Local, like timeline-storage's DynamoDB tests;
//! fails (never skips) without them.

#[path = "../../timeline-storage/tests/support/dynamodb_local.rs"]
mod dynamodb_local;
#[path = "../../timeline-storage/tests/support/s3_local.rs"]
#[allow(dead_code)]
mod s3_local;

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::Router;
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::{json, Value};
use timeline_api::app::build_router;
use timeline_api::aws_settings::{AwsSettings, MissingSettings};
use timeline_api::aws_state::{build_aws_state, fetch_jwks, AwsClients};
use timeline_api::dev_only::{generate_dev_keypair, DEV_KEYPAIR};
use timeline_api::flag_handles::FlagHandleKey;
use timeline_core::model::ConversationId;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::raw_object_key;
use tower::ServiceExt;

const FIXTURE: &str = include_str!("../../timeline-core/tests/fixtures/sample_conversations.json");
const KID: &str = "dev-only-key-1"; // generate_dev_keypair's fixed key id
const KEY_VALUE: &str = "a-test-flag-handle-key-of-more-than-32-bytes";

// ---- Settings ------------------------------------------------------------

fn full_env() -> HashMap<&'static str, String> {
    HashMap::from([
        ("TIMELINE_UPLOADS_BUCKET", "uploads".to_string()),
        ("TIMELINE_CONVERSATIONS_TABLE", "conversations".to_string()),
        (
            "TIMELINE_COGNITO_USER_POOL_ID",
            "us-east-1_TestPool".to_string(),
        ),
        ("TIMELINE_COGNITO_CLIENT_ID", "test-client".to_string()),
        ("AWS_REGION", "us-east-1".to_string()),
    ])
}

fn settings_from(env: &HashMap<&'static str, String>) -> Result<AwsSettings, MissingSettings> {
    AwsSettings::from_lookup(|name| env.get(name).cloned())
}

#[test]
fn every_setting_is_read_and_the_cognito_addresses_follow_from_them() {
    let s = settings_from(&full_env()).unwrap();
    assert_eq!(s.uploads_bucket.as_str(), "uploads");
    assert_eq!(s.conversations_table.as_str(), "conversations");
    assert_eq!(s.client_id.as_str(), "test-client");
    assert_eq!(
        s.issuer(),
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool"
    );
    assert_eq!(
        s.jwks_url(),
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool/.well-known/jwks.json"
    );
}

#[test]
fn each_missing_setting_is_named() {
    for name in full_env().keys() {
        let mut env = full_env();
        env.remove(name);
        assert_eq!(
            settings_from(&env),
            Err(MissingSettings(vec![*name])),
            "{name}"
        );
    }
}

#[test]
fn an_empty_or_blank_setting_counts_as_missing() {
    let mut env = full_env();
    env.insert("TIMELINE_UPLOADS_BUCKET", String::new());
    env.insert("AWS_REGION", "   ".to_string());
    assert_eq!(
        settings_from(&env),
        Err(MissingSettings(vec![
            "TIMELINE_UPLOADS_BUCKET",
            "AWS_REGION"
        ]))
    );
}

#[test]
fn all_missing_settings_are_reported_at_once() {
    let err = settings_from(&HashMap::new()).err().unwrap();
    // Five since the message-flags table went (plan
    // 2026-10-06-load-only-what-the-page-shows.md §10b).
    assert_eq!(err.0.len(), 5);
    let text = err.to_string();
    for name in full_env().keys() {
        assert!(text.contains(name), "{text} should name {name}");
    }
}

// ---- A local stand-in for Cognito's published keys -----------------------

/// Serves `/jwks.json` (the given key set), `/broken` (a 500) and
/// `/not-json` on a free local port; returns its base address.
async fn serve_keys(jwks: JwkSet) -> String {
    let body = serde_json::to_string(&jwks).unwrap();
    let app = Router::new()
        .route("/jwks.json", get(move || async move { body }))
        .route(
            "/broken",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "down") }),
        )
        .route("/not-json", get(|| async { "<html>not a key set</html>" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("key stand-in stopped: {e}");
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn keys_are_downloaded_from_the_published_address() {
    let (_, jwks) = generate_dev_keypair();
    let base = serve_keys(jwks.clone()).await;
    let fetched = fetch_jwks(&format!("{base}/jwks.json")).await.unwrap();
    assert_eq!(fetched.keys.len(), jwks.keys.len());
}

#[tokio::test]
async fn a_failed_key_download_names_the_address_and_the_problem() {
    let (_, jwks) = generate_dev_keypair();
    let base = serve_keys(jwks).await;
    let broken = fetch_jwks(&format!("{base}/broken")).await.err().unwrap();
    assert!(broken.to_string().contains(&format!("{base}/broken")));
    assert!(broken.to_string().contains("500"));
    let not_json = fetch_jwks(&format!("{base}/not-json")).await.err().unwrap();
    assert!(not_json.to_string().contains("not a key set"));
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let unreachable = format!("http://127.0.0.1:{port}/jwks.json");
    let refused = fetch_jwks(&unreachable).await.err().unwrap();
    assert!(refused.to_string().contains(&unreachable));
    assert!(refused.to_string().contains("request failed"));
}

// ---- The Lambda's router against the stand-ins ---------------------------

#[derive(Serialize)]
struct Claims<'a> {
    sub: &'a str,
    iss: &'a str,
    client_id: &'a str,
    token_use: &'a str,
    exp: i64,
}

fn token(pem: &str, sub: &str, iss: &str, client_id: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_string());
    let claims = Claims {
        sub,
        iss,
        client_id,
        token_use: "access",
        exp: 9_999_999_999,
    };
    encode(
        &header,
        &claims,
        &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap(),
    )
    .unwrap()
}

/// Everything one test needs: the stand-ins, the settings pointing at them,
/// and the key pair standing in for the user pool's.
struct World {
    s3: s3_local::LocalS3,
    dynamodb: aws_sdk_dynamodb::Client,
    settings: AwsSettings,
    pool_pem: String,
    pool_jwks: JwkSet,
}

impl World {
    async fn new() -> Self {
        let s3 = s3_local::LocalS3::start().await;
        let dynamodb = dynamodb_local::client();
        let conversations = dynamodb_local::create_table(&dynamodb).await;
        let mut env = full_env();
        env.insert("TIMELINE_UPLOADS_BUCKET", s3_local::BUCKET.to_string());
        env.insert("TIMELINE_CONVERSATIONS_TABLE", conversations);
        let (pool_pem, pool_jwks) = generate_dev_keypair();
        World {
            s3,
            dynamodb,
            settings: settings_from(&env).unwrap(),
            pool_pem,
            pool_jwks,
        }
    }

    /// A router built the way the Lambda builds it. Two calls stand in for
    /// two Lambda instances sharing the same AWS resources and key.
    fn lambda_router(&self) -> Router {
        let clients = AwsClients {
            s3: self.s3.client.clone(),
            dynamodb: self.dynamodb.clone(),
        };
        let key = FlagHandleKey::from_env_value(Some(KEY_VALUE)).unwrap();
        build_router(build_aws_state(
            &self.settings,
            clients,
            self.pool_jwks.clone(),
            key,
        ))
    }

    fn pool_token(&self, sub: &str) -> String {
        token(
            &self.pool_pem,
            sub,
            &self.settings.issuer(),
            self.settings.client_id.as_str(),
        )
    }

    /// Stores the fixture as an upload, through upload processing on the
    /// real adapters: its records, message rows and sessions (plan
    /// 2026-10-06-load-only-what-the-page-shows.md §10b: the export reads
    /// rows now, not the raw file).
    async fn store_fixture_as(&self, user: &str) -> Vec<ConversationId> {
        let user_id = UserId(user.to_string());
        let upload_id = UploadId(uuid::Uuid::new_v4());
        let stores = timeline_api::aws_state::build_processing_stores(
            &self.settings.storage(),
            AwsClients {
                s3: self.s3.client.clone(),
                dynamodb: self.dynamodb.clone(),
            },
        );
        stores
            .object_store
            .put(
                &raw_object_key(&user_id, upload_id),
                FIXTURE.as_bytes().to_vec(),
            )
            .await
            .unwrap();
        stores
            .upload_outcome_store
            .record_received(
                &user_id,
                upload_id,
                timeline_core::conversation_metadata::UploadFacts {
                    file_name: timeline_core::labels::FileName::parse("conversations.json")
                        .unwrap(),
                    uploaded_at: chrono::Utc::now(),
                    file_written_at: None,
                    human_name: timeline_core::labels::PersonName::parse(user).unwrap(),
                },
            )
            .await
            .unwrap();
        timeline_api::processing::process_upload(&stores, &user_id, upload_id)
            .await
            .unwrap();
        stores
            .conversation_summary_store
            .list_for_user(&user_id)
            .await
            .unwrap()
            .iter()
            .map(|s| s.conversation_id)
            .collect()
    }
}

async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, bytes)
}

fn get_with(token: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_token_for_this_pool_reads_conversations_from_dynamodb() {
    let world = World::new().await;
    let ids = world.store_fixture_as("alice").await;
    let (status, body) = call(
        &world.lambda_router(),
        get_with(&world.pool_token("alice"), "/conversations"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // The records are inside the reply in parts (plan §8c).
    let listed: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(listed["conversations"].as_array().unwrap().len(), ids.len());
}

/// The property the in-memory state lacked: a second instance, built from
/// the same settings, sees what the first one stored.
#[tokio::test]
async fn export_and_a_flag_save_on_one_instance_are_visible_on_another() {
    let world = World::new().await;
    world.store_fixture_as("alice").await;
    let token = world.pool_token("alice");
    let first = world.lambda_router();

    // The download comes in the reply itself, in parts (plan §8c); the
    // fixture fits in one.
    let (status, body) = call(&first, get_with(&token, "/export")).await;
    assert_eq!(status, StatusCode::OK);
    let reply: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(reply["cursor"], Value::Null);
    let exported: Value = serde_json::from_str(reply["part"].as_str().unwrap()).unwrap();
    let conversation = exported["conversations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| !c["chat_messages"].as_array().unwrap().is_empty())
        .unwrap();
    let message = conversation["chat_messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["sender"] == "human")
        .unwrap();
    let (conv, msg) = (
        conversation["uuid"].as_str().unwrap(),
        message["uuid"].as_str().unwrap(),
    );
    let handle = reply["flag_handles"][msg].as_str().unwrap();

    let patch = Request::builder()
        .method("PATCH")
        .uri(format!("/conversations/{conv}/messages/{msg}/flags"))
        .header("Authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"angry": true, "handle": handle}).to_string(),
        ))
        .unwrap();
    let (status, _) = call(&first, patch).await;
    assert_eq!(status, StatusCode::OK);

    let second = world.lambda_router();
    let (status, body) = call(
        &second,
        get_with(
            &token,
            &format!("/conversations/{conv}/messages/{msg}/flags"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let record: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(record["user"]["angry"], json!(true));
}

#[tokio::test]
async fn a_token_signed_with_the_dev_keys_is_refused() {
    let world = World::new().await;
    let (dev_pem, _) = &*DEV_KEYPAIR;
    let forged = token(
        dev_pem,
        "alice",
        &world.settings.issuer(),
        world.settings.client_id.as_str(),
    );
    let (status, _) = call(&world.lambda_router(), get_with(&forged, "/conversations")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_token_for_another_client_or_issuer_is_refused() {
    let world = World::new().await;
    let router = world.lambda_router();
    let other_client = token(
        &world.pool_pem,
        "alice",
        &world.settings.issuer(),
        "another-client",
    );
    let (status, _) = call(&router, get_with(&other_client, "/conversations")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let other_issuer = token(
        &world.pool_pem,
        "alice",
        "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_OtherPool",
        world.settings.client_id.as_str(),
    );
    let (status, _) = call(&router, get_with(&other_issuer, "/conversations")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
