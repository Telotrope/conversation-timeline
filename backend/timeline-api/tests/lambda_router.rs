//! Asserts that the `_dev` local-testing routes are absent from the router
//! the Lambda build serves.
//!
//! `main.rs` merges `build_dev_router` only in the local branch, so the
//! claim is structural rather than conditional -- but nothing verified it,
//! and `main.rs` itself has no test coverage at all. This is the one
//! property in this codebase where being wrong means shipping an
//! unauthenticated token-minting endpoint (`POST /_dev/login`) and, since
//! the reset route landed, an unauthenticated "erase everything"
//! endpoint, to production.
//!
//! So: build exactly what the Lambda branch builds, and check every `_dev`
//! path answers 404.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use timeline_api::app::build_router;
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use tower::ServiceExt;

/// Exactly the state `main.rs` hands `build_router` on the Lambda path.
fn lambda_state() -> AppState {
    let (_, jwks) = &*DEV_KEYPAIR;
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store: Arc<dyn ObjectStore> = Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store: Arc<dyn ConversationSummaryStore> =
        Arc::new(InMemoryConversationSummaryStore::new());
    AppState {
        object_store,
        conversation_summary_store,
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store,
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
    }
}

async fn status_of(method: &str, uri: &str) -> StatusCode {
    let router = build_router(lambda_state());
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    router.oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn the_lambda_router_serves_no_dev_routes() {
    for (method, uri) in [
        ("POST", "/_dev/login"),
        ("POST", "/_dev/reset"),
        ("PUT", "/_dev/local-storage/put/raw/alice/x.json"),
        ("GET", "/_dev/local-storage/get/raw/alice/x.json"),
    ] {
        assert_eq!(
            status_of(method, uri).await,
            StatusCode::NOT_FOUND,
            "{method} {uri} must not exist in the Lambda build"
        );
    }
}

#[tokio::test]
async fn the_lambda_router_still_serves_the_real_routes() {
    // The negative test above would also pass on an empty router, which
    // would prove nothing. This pins the other side: the real routes are
    // present and merely rejecting the request for want of a token.
    for (method, uri) in [
        ("POST", "/uploads"),
        ("GET", "/conversations"),
        ("GET", "/export"),
        ("POST", "/detect"),
    ] {
        let status = status_of(method, uri).await;
        assert_ne!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {uri} should exist in the Lambda build"
        );
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "{method} {uri} should reject an unauthenticated request"
        );
    }
}

#[tokio::test]
async fn reset_empties_the_stores_it_is_given() {
    // Exercised through the dev router, which is where it actually lives.
    use axum::Router;
    use timeline_api::app::build_dev_router;
    use timeline_api::dev_state::DevState;
    use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store_concrete = Arc::new(InMemoryObjectStore::new());
    let summaries_concrete = Arc::new(InMemoryConversationSummaryStore::new());
    let outcomes = Arc::new(InMemoryUploadOutcomeStore::new());

    let dev_state = DevState {
        object_store: object_store_concrete.clone(),
        upload_outcome_store: outcomes.clone(),
        conversation_summary_store: summaries_concrete.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        resettable: Arc::new(vec![
            object_store_concrete.clone(),
            summaries_concrete.clone(),
            flags_store,
            outcomes,
        ]),
    };
    let router: Router = build_dev_router(dev_state);

    // Put something in, through the store's own port rather than a back door.
    object_store_concrete
        .put("raw/alice/thing.json", b"hello".to_vec())
        .await
        .unwrap();
    assert!(object_store_concrete.get("raw/alice/thing.json").await.is_ok());

    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/_dev/reset")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    assert!(
        object_store_concrete.get("raw/alice/thing.json").await.is_err(),
        "reset should have discarded the stored object"
    );
}
