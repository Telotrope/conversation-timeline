//! Black-box tests for the `_dev`-only local-testing routes: minting a
//! token via `POST /_dev/login`, uploading through
//! `_dev/local-storage/put/{*key}` (which stores bytes exactly like a real
//! presigned PUT would, and triggers the same processing a real S3 event
//! would), and downloading through `_dev/local-storage/get/{*key}`. All
//! through real HTTP requests against the merged router -- the same shape
//! `main.rs` serves in local dev. See the migration plan's §V2a.

#[path = "support/local_app.rs"]
mod local_app;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

/// Mirrors `main.rs::build_local_state` exactly: the same process-wide
/// generated dev-only keypair, and the same underlying stores shared
/// between `AppState` and `DevState` -- otherwise an upload PUT through the
/// dev router would never be visible to `GET /conversations` on the app
/// router.
fn test_router() -> Router {
    local_app::router()
}

async fn body_json(response: axum::response::Response) -> Value {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn dev_login(router: &Router, sub: &str) -> String {
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": sub }).to_string()))
        .unwrap();
    let response = router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await["token"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn dev_login_mints_a_token_the_real_api_accepts() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    let request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn dev_login_rejects_an_empty_sub() {
    let router = test_router();
    let request = Request::builder()
        .method("POST")
        .uri("/_dev/login")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "sub": "" }).to_string()))
        .unwrap();
    let response = router.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn putting_a_raw_upload_through_local_storage_triggers_processing() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    // POST /uploads first, exactly like the real fetch()-based upload flow
    // will -- this is what actually produces the upload_url to PUT to.
    let create_request = Request::builder()
        .method("POST")
        .uri("/uploads")
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .body(Body::from(
            r#"{"file_name":"conversations.json","human_name":"Alice"}"#,
        ))
        .unwrap();
    let create_response = router.clone().oneshot(create_request).await.unwrap();
    assert_eq!(create_response.status(), StatusCode::OK);
    let upload_url = body_json(create_response).await["upload_url"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(upload_url.starts_with("/_dev/local-storage/put/"));

    let raw = r#"[{"uuid":"11111111-1111-4111-8111-111111111111","name":"Hi","chat_messages":[{"uuid":"22222222-2222-4222-8222-222222222222","sender":"human","created_at":"2024-01-01T00:00:00Z","content":[{"type":"text","text":"WRONG, you failed to fix it."}]}]}]"#;
    let put_request = Request::builder()
        .method("PUT")
        .uri(&upload_url)
        .body(Body::from(raw))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let list_request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let list_response = router.clone().oneshot(list_request).await.unwrap();
    // The records are inside the reply in parts (plan
    // 2026-10-06-load-only-what-the-page-shows.md §8c).
    let convs = body_json(list_response).await["conversations"].clone();
    assert_eq!(convs.as_array().unwrap().len(), 1);
    assert_eq!(convs[0]["name"], "Hi");
    assert_eq!(convs[0]["message_count"], 1);
}

#[tokio::test]
async fn a_non_raw_key_stores_bytes_but_does_not_trigger_processing() {
    let router = test_router();
    let token = dev_login(&router, "alice").await;

    let put_request = Request::builder()
        .method("PUT")
        .uri("/_dev/local-storage/put/export/alice/whatever.json")
        .body(Body::from("hello"))
        .unwrap();
    let put_response = router.clone().oneshot(put_request).await.unwrap();
    assert_eq!(put_response.status(), StatusCode::OK);

    let get_request = Request::builder()
        .method("GET")
        .uri("/_dev/local-storage/get/export/alice/whatever.json")
        .body(Body::empty())
        .unwrap();
    let get_response = router.clone().oneshot(get_request).await.unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);
    let bytes = get_response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&bytes[..], b"hello");

    let list_request = Request::builder()
        .method("GET")
        .uri("/conversations")
        .header("Authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap();
    let list_response = router.oneshot(list_request).await.unwrap();
    let convs = body_json(list_response).await["conversations"].clone();
    assert_eq!(convs.as_array().unwrap().len(), 0);
}
