//! Local-dev and Lambda entrypoint, per the migration plan section 6.4.
//! Lambda always sets `AWS_LAMBDA_RUNTIME_API` in its execution
//! environment, so that's what decides which mode to run in.
//!
//! Local mode uses the in-memory storage adapters exclusively -- there is
//! no AWS access in this environment to build real S3/DynamoDB clients
//! against, so this is genuinely "run the whole app with no AWS at all,"
//! not a stand-in for hitting real infrastructure. It's real enough to
//! exercise the full request/response/auth path end-to-end, which is worth
//! having even though it isn't what V2's own test plan calls "done" (that
//! needs LocalStack and a real Cognito pool -- see the migration plan's V2
//! test list and this crate's README).
//!
//! The signing key below is a fixed, checked-in, dev-only RSA keypair --
//! never valid for anything real, and must never be used for an actual
//! deployment. Its only purpose is letting `cargo run` here and a
//! hand-crafted test JWT exercise the whole auth path locally.

use std::sync::Arc;

use axum::Router;
use timeline_api::app::build_router;
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_storage::memory::conversations::InMemoryConversationStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadStore;

pub const DEV_ONLY_JWKS_JSON: &str = include_str!("../dev_only_test_jwks.json");
pub const DEV_ONLY_ISSUER: &str = "https://dev-only.invalid/local-testing";
pub const DEV_ONLY_CLIENT_ID: &str = "dev-only-local-client";

fn build_local_state() -> AppState {
    let jwks =
        serde_json::from_str(DEV_ONLY_JWKS_JSON).expect("dev_only_test_jwks.json is well-formed");
    // Reader and writer must share the *same* underlying store -- two
    // separate `InMemoryMessageFlagsStore`s would each hold their own
    // Mutex<HashMap>, so a PATCH through one would never be visible to a
    // GET through the other. One store, exposed as two differently-typed
    // trait-object handles.
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    AppState {
        object_store: Arc::new(InMemoryObjectStore::new()),
        upload_store: Arc::new(InMemoryUploadStore::new()),
        conversation_store: Arc::new(InMemoryConversationStore::new()),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store,
        verifier: Arc::new(CognitoVerifier::new(
            jwks,
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
    }
}

async fn run_locally(router: Router) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("binding the local dev listener");
    println!("timeline-api (local dev, in-memory storage) listening on http://{addr}");
    axum::serve(listener, router)
        .await
        .expect("local dev server");
}

#[tokio::main]
async fn main() {
    let router = build_router(build_local_state());

    if std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok() {
        lambda_http::run(router).await.expect("lambda runtime");
    } else {
        run_locally(router).await;
    }
}
