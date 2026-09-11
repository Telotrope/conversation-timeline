//! Black-box, end-to-end tests of the whole axum app -- real HTTP
//! request/response cycles through `build_router`, exercised via
//! `tower::ServiceExt::oneshot` (no real TCP listener needed, but every
//! layer -- routing, the auth extractor, JSON (de)serialization, the
//! handler, the in-memory store -- is genuinely exercised, the same as a
//! real request would be). This is the committed version of a manual
//! `cargo run` + `curl` session used during development to confirm the
//! same behavior against a real running server.

use std::sync::{Arc, LazyLock};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::{json, Value};
use timeline_api::app::build_router;
use timeline_api::dev_only::generate_dev_keypair;
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use tower::ServiceExt;

// A throwaway RSA keypair generated once for this whole test binary --
// never written to disk, never checked into git. See
// timeline_api::dev_only's module doc for why a checked-in key (the
// previous design) was a bad idea even though it was never valid for
// anything real.
static TEST_KEYPAIR: LazyLock<(String, JwkSet)> = LazyLock::new(generate_dev_keypair);
const TEST_KID: &str = "dev-only-key-1"; // matches generate_dev_keypair's fixed kid
const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_testpool";
const CLIENT_ID: &str = "test-client-id";

fn test_state() -> AppState {
    let (_, jwks) = &*TEST_KEYPAIR;
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    AppState {
        object_store: Arc::new(InMemoryObjectStore::new()),
        conversation_summary_store: Arc::new(InMemoryConversationSummaryStore::new()),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store,
        verifier: Arc::new(CognitoVerifier::new(jwks.clone(), ISSUER, CLIENT_ID)),
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
    let (pem, _) = &*TEST_KEYPAIR;
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(TEST_KID.to_string());
    let claims = Claims {
        sub,
        iss: ISSUER,
        client_id: CLIENT_ID,
        token_use: "access",
        exp: 9_999_999_999,
    };
    let key = EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();
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
    // Updated from the old "memory://put/" placeholder scheme: the
    // in-memory adapter now returns a real, fetchable local path -- see
    // InMemoryObjectStore's module doc and the migration plan's §V2a --
    // since a real browser needs something it can actually PUT to, not an
    // inert stand-in string. Flagged explicitly per CLAUDE.md's rule on
    // modifying a committed test's assertions.
    assert!(body["upload_url"]
        .as_str()
        .unwrap()
        .starts_with("/_dev/local-storage/put/"));
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
/// `routes::uploads::create_upload` (an `ObjectStore` backend error
/// becoming a 500 response) without needing a real backend that can
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
