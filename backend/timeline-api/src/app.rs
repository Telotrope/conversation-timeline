//! Assembles the axum `Router`s. Kept separate from `main.rs` so the same
//! routers can be exercised directly in tests (via
//! `tower::ServiceExt::oneshot`) without going through either the local
//! dev server or the Lambda runtime.

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post, put};
use axum::Router;

use crate::dev_state::DevState;
use crate::routes::activity::{self, ActivityState};
use crate::routes::{
    analyses, conversations, detect, dev_local_storage, dev_login, dev_reset, export, files, flags,
    messages, metadata, sessions, uploads,
};
use crate::state::AppState;

/// The real, user-facing API -- every route Cognito gates in production,
/// and the only router ever present in the Lambda build.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/uploads",
            post(uploads::create_upload).get(metadata::list_uploads),
        )
        .route("/uploads/{upload_id}", get(uploads::upload_status))
        .route("/uploads/{upload_id}/metadata", put(metadata::edit_upload))
        .route("/conversations", get(conversations::list_conversations))
        .route(
            "/conversations/{conversation_id}/metadata",
            put(metadata::edit_conversation),
        )
        .route(
            "/conversations/{conversation_id}/files",
            get(conversations::list_files),
        )
        .route("/sessions", get(sessions::list_sessions))
        .route("/messages", get(messages::list_messages))
        .route("/analyses/{name}", get(analyses::analysis))
        .route(
            "/files/{conversation_id}/{message_id}/{number}",
            get(files::file_address),
        )
        .route(
            "/conversations/{conversation_id}/messages/{message_id}/flags",
            get(flags::get_flags).patch(flags::patch_flags),
        )
        .route("/export", get(export::export))
        .route("/detect", post(detect::detect))
        .with_state(state)
}

/// `POST /activity` alone: the page's record of what the user did. Its own
/// Lambda function on AWS (`bin/record_activity.rs`); merged into the local
/// server by `main.rs`. See `crate::routes::activity`.
pub fn build_activity_router(state: ActivityState) -> Router {
    Router::new()
        .route("/activity", post(activity::record_activity))
        .with_state(state)
}

/// The `_dev`-only local-testing surface -- see the migration plan's §V2a
/// and `crate::dev_state`'s module doc. `main.rs` merges this into the main
/// router only when running locally, never under Lambda.
pub fn build_dev_router(state: DevState) -> Router {
    Router::new()
        .route(
            "/_dev/local-storage/put/{*key}",
            // axum defaults every request body to a 2MB limit -- fine for
            // API Gateway/Lambda traffic, but this route stands in for a
            // direct-to-S3 upload (see module doc), which in production
            // never passes through this size check at all. Without
            // disabling it here, any real conversations.json export bigger
            // than 2MB (routine -- the project's own real export was
            // 64.7MB) fails with a bare, unhelpful 413. Reproduced and
            // confirmed directly (a 5MB test PUT) before fixing, per
            // CLAUDE.md's rule against fabricated explanations.
            put(dev_local_storage::put_object).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/_dev/local-storage/get/{*key}",
            get(dev_local_storage::get_object),
        )
        // Unlike `/_dev/login` below, this one needs state, so it has to be
        // registered before `.with_state` resolves the router.
        .route("/_dev/reset", post(dev_reset::reset))
        .with_state(state)
        // `/_dev/login` needs no state at all (see `routes::dev_login`), so
        // it's merged in after `.with_state` resolves the router above --
        // a stateless route works with any `Router<S>`.
        .route("/_dev/login", post(dev_login::login))
}
