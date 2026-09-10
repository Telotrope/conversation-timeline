//! Black-box, end-to-end tests of the whole axum app -- real HTTP
//! request/response cycles through `build_router`, exercised via
//! `tower::ServiceExt::oneshot` (no real TCP listener needed, but every
//! layer -- routing, the auth extractor, JSON (de)serialization, the
//! handler, the in-memory store -- is genuinely exercised, the same as a
//! real request would be). This is the committed version of a manual
//! `cargo run` + `curl` session used during development to confirm the
//! same behavior against a real running server.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::{json, Value};
use timeline_api::app::build_router;
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_storage::memory::conversations::InMemoryConversationStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadStore;
use tower::ServiceExt;

// Same throwaway test keypair as timeline-auth/tests/cognito.rs -- see that
// file's comment. Never used for anything real.
const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvwIBADANBgkqhkiG9w0BAQEFAASCBKkwggSlAgEAAoIBAQCj31aKAeOwf1pk
K2LN/lopW9mSXmYnNPxFpG2RyEu4MPXJWSuGkXfiBGdU1hqjrg9rZuuFvSPOnyQJ
/sPJhPx71BtWe9vy7gg5tPIoK6/leV0WCEZOuZfpNEy1gAabSOkrOaiyDfEjw11Q
uadi8+eg/SktgGJhyk2K+RM0AfgLCxZU+e011xshkDZ7JsvskSR65NWrxS7hBWWx
f0IcMRE1XrrDHm9Xt2xNPZZZPkHFkITxlVajZC/6/0yPeQtjiTeclL6CNDblKbAl
VGIIoUIRXtN8vTSe6/PxaELaAPqrGK73ClE13k7ZO8+IlrO9Ww9t9BHq9p+hWNx4
bGQ1mQJfAgMBAAECggEAAVDQtQLcpmpuRt50QuTSsnLu6llC915rRRL7mWyd8u02
U8uQmCnn5Vf9N/OvlJRPoBfWg3NZQniJsN6WGlHrPWMY7c2sPb1Wy9WAkWDXtEqp
6ZjWyc2ZcKK6dchH5JZMhFR19YR0j5QwxRMmvKty3rnYjLOBtf6pxUAYHbst9cyi
5adyJ4s+t0HRED4kupDR8WpXqzz3MS7O6GD9p8AVmSC1ZZY8VOnITcY7Dpb2PR06
RwehMXbdK9b69BVd/rf6IOGAsJefKMJMjiwb87PkhRhJqfRd7p5asrdm27nn8lhQ
ZUVzlkqRmiJ2Ig7d3WMfO8egGFePV9dQAicvfxa6MQKBgQDV7z1E/4vs13PmV7bl
djag5oU/PWGlYakJJtOwLP8mJqLkeum2vdyQaaFd5PIC0ne6FEu8SkIbWxqwmYhR
TVOaVUTb8C17LeIb0tKfFOxI2sX1WStB6PikD5mmAJ4mNOQOWcPXdhKWld83ctr0
xZCXUjLRSMIuth8ZjtYTGcdWxQKBgQDEGCLxUKnqn5EeSgKNsvIbN+cBtu30p+0b
p+7NZ+Td0W7yxIvswo9BTW9QpBkdPrFLvqCdg3c0oYGDI25zYSt+SRz/acNjlpFC
fG+PGp5EyFBXVB8TGVJ8JK7cMpmhrQYfZlbyJNpTARkg4azR8Bw03/WDKcdaRI60
5qY6rnJm0wKBgQCFpsvBQmE5WrTGj7/shLjGNp3CD2fkeSmwVPhlFQdl3zdexEck
amLUOZmdXj2vc6tmre1OuZmpG3aGI7TdDhEP1vuI5/iR/u1GcqQwzFJ9hWesysNS
juhfHnvgEHy848giCwRlpBciyojETFXsG00krC6hPvJJWm/9eJXXIwC8/QKBgQCG
dzae231o0fqlFoMhv6+dUnwqBNKvjeddq45pc/DQ2qiF+JkqxU+OrBbE6YH/N9pD
4ngpCtlXUdiJoGZA4ET+2Av2aQP+6mS5frLRIqOc7u+IsrqMUjTpxA3UGS6YWxlz
tq2wZe0ANiSRE696VnhBGcI1KxT0pUZmbjNW0gDI2QKBgQCztwMys0ILsuhfWrZk
Ee6K5exfbmGTTyuZJY6bOpT/Gn/iPb1oh4cPvQAeMMo+t9WJNUumjUpWv7XPNFom
sLk3FL95Owdyct1cu33lfcm/9/qriAzucNEZ3z4cRA3ivgn4JTxhVJh5K6XLXsl6
yxaADll3PS6Ln8CszrSkfm54Pg==
-----END PRIVATE KEY-----";
const TEST_KEY_N: &str = "o99WigHjsH9aZCtizf5aKVvZkl5mJzT8RaRtkchLuDD1yVkrhpF34gRnVNYao64Pa2brhb0jzp8kCf7DyYT8e9QbVnvb8u4IObTyKCuv5XldFghGTrmX6TRMtYAGm0jpKzmosg3xI8NdULmnYvPnoP0pLYBiYcpNivkTNAH4CwsWVPntNdcbIZA2eybL7JEkeuTVq8Uu4QVlsX9CHDERNV66wx5vV7dsTT2WWT5BxZCE8ZVWo2Qv-v9Mj3kLY4k3nJS-gjQ25SmwJVRiCKFCEV7TfL00nuvz8WhC2gD6qxiu9wpRNd5O2TvPiJazvVsPbfQR6vafoVjceGxkNZkCXw";
const TEST_KID: &str = "test-key-1";
const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_testpool";
const CLIENT_ID: &str = "test-client-id";

fn test_state() -> AppState {
    let jwks: JwkSet = serde_json::from_value(json!({
        "keys": [{"kty": "RSA", "kid": TEST_KID, "use": "sig", "alg": "RS256", "n": TEST_KEY_N, "e": "AQAB"}]
    }))
    .unwrap();
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    AppState {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_store: Arc::new(InMemoryUploadStore::new()),
        conversation_store: Arc::new(InMemoryConversationStore::new()),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store,
        verifier: Arc::new(CognitoVerifier::new(jwks, ISSUER, CLIENT_ID)),
    }
}

#[derive(Serialize)]
struct Claims<'a> {
    sub: &'a str,
    iss: &'a str,
    client_id: &'a str,
    token_use: &'a str,
    exp: i64,
}

fn test_token(sub: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_KID.to_string());
    let claims = Claims {
        sub,
        iss: ISSUER,
        client_id: CLIENT_ID,
        token_use: "access",
        exp: 9_999_999_999,
    };
    let key = EncodingKey::from_rsa_pem(TEST_KEY_PEM.as_bytes()).unwrap();
    encode(&header, &claims, &key).unwrap()
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn unauthenticated_request_is_rejected() {
    let router = build_router(test_state());
    let request = Request::builder()
        .uri("/conversations")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn garbage_bearer_token_is_rejected() {
    let router = build_router(test_state());
    let request = Request::builder()
        .uri("/conversations")
        .header("Authorization", "Bearer not-a-real-token")
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn list_conversations_starts_empty() {
    let router = build_router(test_state());
    let request = Request::builder()
        .uri("/conversations")
        .header("Authorization", format!("Bearer {}", test_token("alice")))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await, json!([]));
}

#[tokio::test]
async fn create_upload_returns_an_id_and_a_presigned_url() {
    let router = build_router(test_state());
    let request = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {}", test_token("alice")))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert!(body["upload_id"].is_string());
    assert!(body["upload_url"]
        .as_str()
        .unwrap()
        .starts_with("memory://put/"));
}

#[tokio::test]
async fn getting_flags_that_were_never_set_is_404() {
    let router = build_router(test_state());
    let conv = "11111111-1111-4111-8111-111111111111";
    let msg = "22222222-2222-4222-8222-222222222222";
    let request = Request::builder()
        .uri(format!("/conversations/{conv}/messages/{msg}/flags"))
        .header("Authorization", format!("Bearer {}", test_token("alice")))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_then_get_flags_round_trips_through_real_http_requests() {
    let router = build_router(test_state());
    let conv = "11111111-1111-4111-8111-111111111111";
    let msg = "22222222-2222-4222-8222-222222222222";
    let token = test_token("alice");

    let patch_request = Request::builder()
        .method("PATCH")
        .uri(format!("/conversations/{conv}/messages/{msg}/flags"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(json!({"caps": true}).to_string()))
        .unwrap();
    let patch_response = router.clone().oneshot(patch_request).await.unwrap();
    assert_eq!(patch_response.status(), StatusCode::OK);
    let patched = body_json(patch_response).await;
    assert_eq!(patched["user"]["caps"], json!(true));
    assert_eq!(
        patched["auto"],
        json!({"caps": false, "critical": false, "angry": false}),
        "a PATCH must never set an auto flag"
    );

    let get_request = Request::builder()
        .uri(format!("/conversations/{conv}/messages/{msg}/flags"))
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let get_response = router.oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(
        body_json(get_response).await,
        patched,
        "a GET right after a PATCH must see the same data"
    );
}

#[tokio::test]
async fn conversations_are_isolated_per_authenticated_user() {
    let router = build_router(test_state());
    // alice's upload must not be visible to bob, even against the same
    // running app / shared in-memory store.
    let create = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {}", test_token("alice")))
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(create).await.unwrap();

    let bob_list = Request::builder()
        .uri("/conversations")
        .header("Authorization", format!("Bearer {}", test_token("bob")))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(bob_list).await.unwrap();
    assert_eq!(body_json(response).await, json!([]));
}

/// A test double that always fails -- proves the error-handling branch in
/// `routes::uploads::create_upload` (an ObjectStore/UploadStore backend
/// error becoming a 500 response) without needing a real backend that can
/// actually fail.
struct FaultyObjectStore;

#[async_trait::async_trait]
impl timeline_core::ports::object_store::ObjectStore for FaultyObjectStore {
    async fn presign_put(
        &self,
        _key: &str,
        _expires_in: std::time::Duration,
    ) -> Result<String, timeline_core::ports::errors::ObjectStoreError> {
        Err(timeline_core::ports::errors::ObjectStoreError::Backend(
            "presigning is down".into(),
        ))
    }
    async fn presign_get(
        &self,
        _key: &str,
        _expires_in: std::time::Duration,
    ) -> Result<String, timeline_core::ports::errors::ObjectStoreError> {
        unimplemented!("not exercised by this test")
    }
    async fn get(
        &self,
        _key: &str,
    ) -> Result<Vec<u8>, timeline_core::ports::errors::ObjectStoreError> {
        unimplemented!("not exercised by this test")
    }
    async fn put(
        &self,
        _key: &str,
        _data: Vec<u8>,
    ) -> Result<(), timeline_core::ports::errors::ObjectStoreError> {
        unimplemented!("not exercised by this test")
    }
}

#[tokio::test]
async fn a_failing_object_store_surfaces_as_a_500_not_a_panic_or_silent_success() {
    let mut state = test_state();
    state.object_store = std::sync::Arc::new(FaultyObjectStore);
    let router = build_router(state);

    let request = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {}", test_token("alice")))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_json(response).await;
    assert_eq!(body["error"], json!("object store backend error"));
}
