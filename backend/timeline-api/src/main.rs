//! Local-dev and Lambda entrypoint, per the migration plan section 6.4.
//! Lambda always sets `AWS_LAMBDA_RUNTIME_API` in its execution
//! environment, so that's what decides which mode to run in.
//!
//! Local mode uses the in-memory storage adapters exclusively -- there is
//! no AWS access in this environment to build real S3/DynamoDB clients
//! against, so this is genuinely "run the whole app with no AWS at all,"
//! not a stand-in for hitting real infrastructure. It's real enough to
//! exercise the full request/response/auth/upload-processing/export path
//! end-to-end (see the migration plan's §V2a), which is worth having even
//! though it isn't what V2's own test plan calls "done" (that needs
//! LocalStack and a real Cognito pool -- see the migration plan's V2 test
//! list and this crate's README).
//!
//! **The Lambda build never gets the `_dev` router.** Only `run_locally`
//! ever calls `build_dev_router` -- the `lambda_http::run` branch is handed
//! `build_router(app_state)` alone, so `AutoFlagWriter`, `UploadOutcomeStore`,
//! and the `_dev/local-storage`/`_dev/login` routes are structurally absent
//! from anything that could run in production, not just conventionally
//! unused.
//!
//! The signing key in [`timeline_api::dev_only::DEV_KEYPAIR`] is generated
//! fresh, in memory, once per process -- never written to disk, never valid
//! for anything real, and must never be used for an actual deployment. Its
//! only purpose is letting `cargo run` here, `POST /_dev/login`, and the
//! test suite exercise the whole auth path locally.

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use timeline_api::app::{build_dev_router, build_router};
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::dev_state::DevState;
use timeline_api::state::AppState;
use timeline_auth::cognito::CognitoVerifier;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

/// Builds the in-memory stores once and exposes them as both `AppState`
/// (the real, Cognito-gated API) and `DevState` (the `_dev`-only local
/// testing surface) -- sharing the same underlying `Arc`s is what lets an
/// upload PUT through `_dev/local-storage` show up in `GET /conversations`.
/// `UploadOutcomeStore` is built for `DevState` only -- no route reachable
/// from `AppState` needs it (see `state::AppState`'s module doc).
fn build_local_state() -> (AppState, DevState) {
    // Forces DEV_KEYPAIR's generation to happen here, up front, rather than
    // lazily on the first login/verification -- so a slow key-generation
    // hiccup shows up at startup, not on some later request.
    let (_, jwks) = &*DEV_KEYPAIR;
    // Reader and writer must share the *same* underlying store -- two
    // separate `InMemoryMessageFlagsStore`s would each hold their own
    // Mutex<HashMap>, so a PATCH through one would never be visible to a
    // GET through the other. One store, exposed as differently-typed
    // trait-object handles.
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store: Arc<dyn timeline_core::ports::object_store::ObjectStore> =
        Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store: Arc<
        dyn timeline_core::ports::conversations::ConversationSummaryStore,
    > = Arc::new(InMemoryConversationSummaryStore::new());

    let app_state = AppState {
        object_store: object_store.clone(),
        conversation_summary_store: conversation_summary_store.clone(),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        verifier: Arc::new(CognitoVerifier::new(jwks.clone(), DEV_ONLY_ISSUER, DEV_ONLY_CLIENT_ID)),
    };
    let dev_state = DevState {
        object_store,
        upload_outcome_store: Arc::new(InMemoryUploadOutcomeStore::new()),
        conversation_summary_store,
        auto_flag_writer: flags_store,
    };
    (app_state, dev_state)
}

async fn run_locally(router: Router) {
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("binding the local dev listener");
    println!("timeline-api (local dev, in-memory storage) listening on http://{addr}");
    println!("_dev-only routes active: POST /_dev/login, /_dev/local-storage/{{put,get}}/*key");
    axum::serve(listener, router)
        .await
        .expect("local dev server");
}

#[tokio::main]
async fn main() {
    if std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok() {
        // `dev_state` is built but deliberately dropped unused here -- the
        // Lambda branch only ever passes `app_state` to `build_router`, so
        // the `_dev` router (and the `AutoFlagWriter` capability it needs)
        // is never wired into anything that could run in production.
        let (app_state, dev_state) = build_local_state();
        drop(dev_state);
        let router = build_router(app_state);
        lambda_http::run(router).await.expect("lambda runtime");
    } else {
        let (app_state, dev_state) = build_local_state();
        // Permissive CORS, local-dev only -- timeline.html isn't served by
        // this binary and will be opened separately (a local file, or a
        // static server on a different port), so without this the browser
        // blocks every cross-origin fetch(). Never applied to the Lambda
        // branch above -- that router is returned before this layer exists.
        let router = build_router(app_state)
            .merge(build_dev_router(dev_state))
            .layer(CorsLayer::permissive());
        run_locally(router).await;
    }
}
