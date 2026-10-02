//! Local-dev and Lambda entrypoint, per the migration plan section 6.4.
//! Lambda always sets `AWS_LAMBDA_RUNTIME_API` in its execution
//! environment, so that's what decides which mode to run in.
//!
//! **Lambda mode** uses the real S3 and DynamoDB adapters and checks logins
//! against the Cognito user pool's published keys (migration plan §V2d),
//! all built by [`timeline_api::aws_state::build_aws_state`]. A missing
//! setting, a missing flag-handle key, or keys that can't be downloaded
//! stop startup with a message; there is no fallback to anything local.
//!
//! **Local mode** uses the in-memory storage adapters exclusively, so the
//! whole app runs with no AWS at all. It's real enough to
//! exercise the full request/response/auth/upload-processing/export path
//! end-to-end (see the migration plan's §V2a), which is worth having even
//! though it isn't what V2's own test plan calls "done" (that needs
//! LocalStack and a real Cognito pool -- see the migration plan's V2 test
//! list and this crate's README).
//!
//! **The Lambda build never gets the `_dev` router.** Only `run_locally`
//! ever calls `build_dev_router` -- the `lambda_http::run` branch is handed
//! `build_router(app_state)` alone, so the `_dev/local-storage`,
//! `_dev/login` and `_dev/reset` routes are structurally absent from
//! anything that could run in production, not just conventionally unused.
//!
//! The signing key in [`timeline_api::dev_only::DEV_KEYPAIR`] is generated
//! fresh, in memory, once per process -- never written to disk, never valid
//! for anything real, and must never be used for an actual deployment. Its
//! only purpose is letting `cargo run` here, `POST /_dev/login`, and the
//! test suite exercise the whole auth path locally.

use std::sync::Arc;

use axum::Router;
use tower_http::cors::CorsLayer;

use timeline_api::app::{build_activity_router, build_dev_router, build_router};
use timeline_api::aws_call_counter;
use timeline_api::aws_settings::AwsSettings;
use timeline_api::aws_state::{build_aws_state, fetch_jwks, AwsClients};
use timeline_api::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use timeline_api::dev_state::DevState;
use timeline_api::flag_handles::{FlagHandleKey, KEY_ENV_VAR};
use timeline_api::request_log::{stdout_sink, with_request_log};
use timeline_api::routes::activity::ActivityState;
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
/// `UploadOutcomeStore` is shared too: the local upload route writes
/// outcomes and `GET /uploads/{upload_id}` reads them.
fn build_local_state(flag_handle_key: FlagHandleKey) -> (AppState, DevState) {
    // Forces DEV_KEYPAIR's generation to happen here, up front, rather than
    // lazily on the first login/verification -- so a slow key-generation
    // hiccup shows up at startup, not on some later request.
    let (_, jwks) = &*DEV_KEYPAIR;
    // Reader and writer must share the *same* underlying store -- two
    // separate `InMemoryMessageFlagsStore`s would each hold their own
    // Mutex<HashMap>, so a PATCH through one would never be visible to a
    // GET through the other. One store, exposed as differently-typed
    // trait-object handles.
    //
    // Each store is built once as its concrete type and then handed out as
    // whichever trait handles need it. Keeping the concrete `Arc` is what
    // lets the same object also appear in `DevState::resettable`: a
    // `Arc<dyn ObjectStore>` cannot be turned back into an
    // `Arc<dyn Resettable>`, so the coercion has to happen from the concrete
    // value, not after the fact.
    let flags_store = Arc::new(InMemoryMessageFlagsStore::new());
    let object_store_concrete = Arc::new(InMemoryObjectStore::new());
    let conversation_summary_store_concrete = Arc::new(InMemoryConversationSummaryStore::new());
    let object_store: Arc<dyn timeline_core::ports::object_store::ObjectStore> =
        object_store_concrete.clone();
    let conversation_summary_store: Arc<
        dyn timeline_core::ports::conversations::ConversationSummaryStore,
    > = conversation_summary_store_concrete.clone();

    // Shared like the flag store: the local upload route records outcomes
    // that `GET /uploads/{upload_id}` reads.
    let upload_outcome_store = Arc::new(InMemoryUploadOutcomeStore::new());
    let app_state = AppState {
        object_store: object_store.clone(),
        conversation_summary_store: conversation_summary_store.clone(),
        flags_reader: flags_store.clone(),
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        upload_outcome_store: upload_outcome_store.clone(),
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
        flag_handle_key: Arc::new(flag_handle_key),
    };
    let dev_state = DevState {
        object_store,
        upload_outcome_store: upload_outcome_store.clone(),
        conversation_summary_store,
        user_flag_writer: flags_store.clone(),
        auto_flag_writer: flags_store.clone(),
        // Same underlying objects as the port handles above, held again as
        // the one capability that is not a storage port -- see
        // `timeline_storage::memory::resettable`.
        resettable: Arc::new(vec![
            object_store_concrete,
            conversation_summary_store_concrete,
            flags_store,
            upload_outcome_store,
        ]),
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
        // Counts each request's AWS calls for its log line (crate::request_log).
        aws_call_counter::install();
        // Every startup problem stops the Lambda with a message naming it;
        // nothing falls back to the in-memory stores or the dev keys, which
        // the Lambda used before §V2d.
        let settings = AwsSettings::from_lookup(|name| std::env::var(name).ok())
            .unwrap_or_else(|e| panic!("cannot start: {e}"));
        // The flag-handle key must come from the environment: a key
        // generated per instance would make Lambda instances reject each
        // other's handles. See `timeline_api::flag_handles`.
        let key = FlagHandleKey::from_env_value(std::env::var(KEY_ENV_VAR).ok().as_deref())
            .unwrap_or_else(|e| panic!("cannot start: {e}"));
        let jwks = fetch_jwks(&settings.jwks_url())
            .await
            .unwrap_or_else(|e| panic!("cannot start: {e}"));
        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let clients = AwsClients {
            s3: aws_sdk_s3::Client::new(&sdk_config),
            dynamodb: aws_sdk_dynamodb::Client::new(&sdk_config),
        };
        let router = build_router(build_aws_state(&settings, clients, jwks, key));
        lambda_http::run(with_request_log(router, stdout_sink()))
            .await
            .expect("lambda runtime");
    } else {
        let (app_state, dev_state) = build_local_state(FlagHandleKey::generate());
        // The page's activity reports, logged on standard output as on AWS,
        // where the route has its own function (bin/record_activity.rs).
        let activity_state = ActivityState {
            verifier: app_state.verifier.clone(),
            sink: stdout_sink(),
        };
        // Permissive CORS, local-dev only -- timeline.html isn't served by
        // this binary and will be opened separately (a local file, or a
        // static server on a different port), so without this the browser
        // blocks every cross-origin fetch(). Never applied to the Lambda
        // branch above -- that router is returned before this layer exists.
        let router = build_router(app_state)
            .merge(build_activity_router(activity_state))
            .merge(build_dev_router(dev_state));
        // Every request logged, as on AWS (docs/plans/2026-10-02-activity-instrumentation.md §3).
        let router = with_request_log(router, stdout_sink()).layer(CorsLayer::permissive());
        run_locally(router).await;
    }
}
