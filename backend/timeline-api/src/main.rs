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
//! The local stores are built by [`timeline_api::local_state`], where tests
//! build the same ones.
//!
//! The signing key in [`timeline_api::dev_only::DEV_KEYPAIR`] is generated
//! fresh, in memory, once per process -- never written to disk, never valid
//! for anything real, and must never be used for an actual deployment. Its
//! only purpose is letting `cargo run` here, `POST /_dev/login`, and the
//! test suite exercise the whole auth path locally.

use axum::Router;
use tower_http::cors::CorsLayer;

use timeline_api::app::{build_activity_router, build_dev_router, build_router};
use timeline_api::aws_call_counter;
use timeline_api::aws_settings::AwsSettings;
use timeline_api::aws_state::{build_aws_state, fetch_jwks, AwsClients};
use timeline_api::flag_handles::{FlagHandleKey, KEY_ENV_VAR};
use timeline_api::local_state::build_local_state;
use timeline_api::request_log::{stdout_sink, with_request_log};
use timeline_api::routes::activity::ActivityState;

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
        // Tests start the local server with a step budget, so small test
        // data still answers in several parts (plan
        // docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c).
        let budget = timeline_api::local_state::budget_from(
            std::env::var(timeline_api::local_state::BUDGET_STEPS_VAR)
                .ok()
                .as_deref(),
        )
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
        let (app_state, dev_state) = build_local_state(FlagHandleKey::generate(), budget);
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
        // Every request logged, as on AWS (docs/plans/completed/2026-10-02-activity-instrumentation.md §3).
        let router = with_request_log(router, stdout_sink()).layer(CorsLayer::permissive());
        run_locally(router).await;
    }
}
