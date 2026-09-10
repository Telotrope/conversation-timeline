//! The axum app's shared state, and the `FromRef` impls that let each route
//! handler declare only the one narrow capability it needs (e.g.
//! `State<Arc<dyn UserFlagWriter>>`) rather than the whole state -- this is
//! what makes the auto/user flag separation from the migration plan
//! section 4.1 real at the wiring level, not just at the trait-definition
//! level: nothing named `Arc<dyn AutoFlagWriter>` is ever in scope inside a
//! user-facing route handler's function body, because it's never a
//! parameter of any handler reachable from this app. The upload-processing
//! Lambda (which *does* need `AutoFlagWriter`) is a separate binary with
//! its own, smaller state -- see `src/bin/process_upload.rs`.

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::conversations::ConversationStore;
use timeline_core::ports::message_flags::{MessageFlagsReader, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadStore;

#[derive(Clone)]
pub struct AppState {
    pub object_store: Arc<dyn ObjectStore>,
    pub upload_store: Arc<dyn UploadStore>,
    pub conversation_store: Arc<dyn ConversationStore>,
    pub flags_reader: Arc<dyn MessageFlagsReader>,
    pub user_flag_writer: Arc<dyn UserFlagWriter>,
    pub verifier: Arc<CognitoVerifier>,
}

impl FromRef<AppState> for Arc<dyn ObjectStore> {
    fn from_ref(state: &AppState) -> Self {
        state.object_store.clone()
    }
}

impl FromRef<AppState> for Arc<dyn UploadStore> {
    fn from_ref(state: &AppState) -> Self {
        state.upload_store.clone()
    }
}

impl FromRef<AppState> for Arc<dyn ConversationStore> {
    fn from_ref(state: &AppState) -> Self {
        state.conversation_store.clone()
    }
}

impl FromRef<AppState> for Arc<dyn MessageFlagsReader> {
    fn from_ref(state: &AppState) -> Self {
        state.flags_reader.clone()
    }
}

impl FromRef<AppState> for Arc<dyn UserFlagWriter> {
    fn from_ref(state: &AppState) -> Self {
        state.user_flag_writer.clone()
    }
}

impl FromRef<AppState> for Arc<CognitoVerifier> {
    fn from_ref(state: &AppState) -> Self {
        state.verifier.clone()
    }
}
