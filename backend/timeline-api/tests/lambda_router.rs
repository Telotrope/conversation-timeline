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

use axum::body::Body;
use axum::http::{Request, StatusCode};
use timeline_api::app::build_router;
use timeline_api::state::AppState;
use tower::ServiceExt;

/// Exactly the state `main.rs` hands `build_router` on the Lambda path.
fn lambda_state() -> AppState {
    timeline_api::local_state::build_local_state(
        timeline_api::flag_handles::FlagHandleKey::generate(),
        timeline_core::work_budget::BudgetSetting::Clock(
            timeline_core::work_budget::REQUEST_WORK_LIMIT,
        ),
    )
    .0
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

    let (_, dev_state) = timeline_api::local_state::build_local_state(
        timeline_api::flag_handles::FlagHandleKey::generate(),
        timeline_core::work_budget::BudgetSetting::Clock(
            timeline_core::work_budget::REQUEST_WORK_LIMIT,
        ),
    );
    let object_store_concrete = dev_state.processing.object_store.clone();
    let router: Router = build_dev_router(dev_state);

    // Put something in, through the store's own port rather than a back door.
    object_store_concrete
        .put("raw/alice/thing.json", b"hello".to_vec())
        .await
        .unwrap();
    assert!(object_store_concrete
        .get("raw/alice/thing.json")
        .await
        .is_ok());

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
        object_store_concrete
            .get("raw/alice/thing.json")
            .await
            .is_err(),
        "reset should have discarded the stored object"
    );
}
