//! The activity-recording Lambda: answers `POST /activity`, the page's
//! record of what the user did (the template's `ActivityFunction`;
//! docs/plans/completed/2026-10-02-activity-instrumentation.md §5). Wiring only; the
//! handler is `timeline_api::routes::activity`, tested in `tests/activity.rs`.
//!
//! Its own function, not the API's, so a report never occupies an API copy
//! that one of the user's requests then has to wait for. It checks logins
//! like the API, from the same Cognito settings; a missing setting or keys
//! that can't be downloaded stop start-up with a message naming them.

use std::sync::Arc;

use timeline_api::app::build_activity_router;
use timeline_api::aws_settings::LoginSettings;
use timeline_api::aws_state::fetch_jwks;
use timeline_api::request_log::{stdout_sink, with_request_log};
use timeline_api::routes::activity::ActivityState;
use timeline_auth::cognito::CognitoVerifier;

#[tokio::main]
async fn main() {
    let settings = LoginSettings::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let jwks = fetch_jwks(&settings.jwks_url())
        .await
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let state = ActivityState {
        verifier: Arc::new(CognitoVerifier::new(
            jwks,
            settings.issuer(),
            settings.client_id.as_str(),
        )),
        sink: stdout_sink(),
    };
    let router = with_request_log(build_activity_router(state), stdout_sink());
    lambda_http::run(router).await.expect("lambda runtime");
}
