//! Assembles the axum `Router`. Kept separate from `main.rs` so the same
//! router can be exercised directly in tests (via `tower::ServiceExt::oneshot`)
//! without going through either the local dev server or the Lambda runtime.

use axum::routing::{get, post};
use axum::Router;

use crate::routes::{conversations, flags, uploads};
use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/uploads", post(uploads::create_upload))
        .route("/conversations", get(conversations::list_conversations))
        .route(
            "/conversations/{conversation_id}/messages/{message_id}/flags",
            get(flags::get_flags).patch(flags::patch_flags),
        )
        .with_state(state)
}
