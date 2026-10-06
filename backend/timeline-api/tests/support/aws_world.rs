//! Shared setup for tests that run the Lambda builds against local
//! stand-ins: S3 served by `s3s-fs`, DynamoDB by DynamoDB Local, and a key
//! pair standing in for the Cognito user pool's. See the migration plan's
//! §V2d and §V2e.
//!
//! A test file using this must also declare the `dynamodb_local` and
//! `s3_local` modules from `timeline-storage/tests/support/` at its root.

use std::collections::HashMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use timeline_api::app::build_router;
use timeline_api::aws_settings::{AwsSettings, StorageSettings};
use timeline_api::aws_state::{build_aws_state, AwsClients};
use timeline_api::dev_only::generate_dev_keypair;
use timeline_api::flag_handles::FlagHandleKey;
use tower::ServiceExt;

use crate::{dynamodb_local, s3_local};

pub const FIXTURE: &str =
    include_str!("../../../timeline-core/tests/fixtures/sample_conversations.json");
const KID: &str = "dev-only-key-1"; // generate_dev_keypair's fixed key id
const KEY_VALUE: &str = "a-test-flag-handle-key-of-more-than-32-bytes";

#[derive(Serialize)]
struct Claims<'a> {
    sub: &'a str,
    iss: &'a str,
    client_id: &'a str,
    token_use: &'a str,
    exp: i64,
}

/// The stand-ins, the settings pointing at them, and the key pair standing
/// in for the user pool's.
pub struct World {
    pub s3: s3_local::LocalS3,
    pub dynamodb: aws_sdk_dynamodb::Client,
    pub settings: AwsSettings,
    pool_pem: String,
    pool_jwks: JwkSet,
}

impl World {
    pub async fn new() -> Self {
        let s3 = s3_local::LocalS3::start().await;
        let dynamodb = dynamodb_local::client();
        let conversations = dynamodb_local::create_table(&dynamodb).await;
        let env = HashMap::from([
            ("TIMELINE_UPLOADS_BUCKET", s3_local::BUCKET.to_string()),
            ("TIMELINE_CONVERSATIONS_TABLE", conversations),
            (
                "TIMELINE_COGNITO_USER_POOL_ID",
                "us-east-1_TestPool".to_string(),
            ),
            ("TIMELINE_COGNITO_CLIENT_ID", "test-client".to_string()),
            ("AWS_REGION", "us-east-1".to_string()),
        ]);
        let settings = AwsSettings::from_lookup(|name| env.get(name).cloned()).unwrap();
        let (pool_pem, pool_jwks) = generate_dev_keypair();
        World {
            s3,
            dynamodb,
            settings,
            pool_pem,
            pool_jwks,
        }
    }

    pub fn clients(&self) -> AwsClients {
        AwsClients {
            s3: self.s3.client.clone(),
            dynamodb: self.dynamodb.clone(),
        }
    }

    /// The storage half of the settings, as the processing Lambda reads it.
    pub fn storage_settings(&self) -> StorageSettings {
        self.settings.storage()
    }

    /// A router built the way the API Lambda builds it.
    pub fn lambda_router(&self) -> Router {
        let key = FlagHandleKey::from_env_value(Some(KEY_VALUE)).unwrap();
        build_router(build_aws_state(
            &self.settings,
            self.clients(),
            self.pool_jwks.clone(),
            key,
        ))
    }

    /// A token the API Lambda accepts for `sub`.
    pub fn pool_token(&self, sub: &str) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KID.to_string());
        let claims = Claims {
            sub,
            iss: &self.settings.issuer(),
            client_id: self.settings.client_id.as_str(),
            token_use: "access",
            exp: 9_999_999_999,
        };
        let key = EncodingKey::from_rsa_pem(self.pool_pem.as_bytes()).unwrap();
        encode(&header, &claims, &key).unwrap()
    }
}

pub async fn call(router: &Router, request: Request<Body>) -> (StatusCode, Vec<u8>) {
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

pub fn get_with(token: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}
