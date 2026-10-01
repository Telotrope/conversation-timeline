//! The axum app's shared state, and the `FromRef` impls that let each route
//! handler declare only the one narrow capability it needs (e.g.
//! `State<Arc<dyn UserFlagWriter>>`) rather than the whole state -- this is
//! what makes the auto/user flag separation from the migration plan
//! section 4.1 real at the wiring level, not just at the trait-definition
//! level.
//!
//! **`AutoFlagWriter` is now part of this state, and that is a deliberate
//! change from the earlier design.** It used to be excluded on the grounds
//! that no user-facing handler should ever be able to write automatic flags,
//! because detection only ever ran in the upload-processing path. Detection
//! is now something the user explicitly asks for (`POST /detect`, see
//! `crate::routes::detect`), so a user-facing route legitimately needs it.
//!
//! What that exclusion was actually protecting is unchanged: automatic and
//! user-confirmed flags remain two separate ports writing two separate
//! stored fields, so an automatic pass still cannot overwrite a flag the
//! user confirmed. The guarantee lives in the port split, not in which
//! struct holds a handle. `UserFlagWriter` and `AutoFlagWriter` are still
//! distinct, and no handler takes both.
//!
//! No user-facing route needs `UploadOutcomeStore`: `POST /uploads` never
//! touches it (there is no pending state to write), and `GET /export`
//! recomputes the raw object's key instead of reading it back, so it's
//! absent from this state entirely, not just unused.

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::{AutoFlagWriter, MessageFlagsReader, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;

use crate::flag_handles::FlagHandleKey;

#[derive(Clone)]
pub struct AppState {
    pub object_store: Arc<dyn ObjectStore>,
    pub conversation_summary_store: Arc<dyn ConversationSummaryStore>,
    pub flags_reader: Arc<dyn MessageFlagsReader>,
    pub user_flag_writer: Arc<dyn UserFlagWriter>,
    pub auto_flag_writer: Arc<dyn AutoFlagWriter>,
    pub verifier: Arc<CognitoVerifier>,
    /// Signs and checks flag handles; see `crate::flag_handles`.
    pub flag_handle_key: Arc<FlagHandleKey>,
}

impl FromRef<AppState> for Arc<dyn ObjectStore> {
    fn from_ref(state: &AppState) -> Self {
        state.object_store.clone()
    }
}

impl FromRef<AppState> for Arc<dyn ConversationSummaryStore> {
    fn from_ref(state: &AppState) -> Self {
        state.conversation_summary_store.clone()
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

impl FromRef<AppState> for Arc<dyn AutoFlagWriter> {
    fn from_ref(state: &AppState) -> Self {
        state.auto_flag_writer.clone()
    }
}

impl FromRef<AppState> for Arc<CognitoVerifier> {
    fn from_ref(state: &AppState) -> Self {
        state.verifier.clone()
    }
}

impl FromRef<AppState> for Arc<FlagHandleKey> {
    fn from_ref(state: &AppState) -> Self {
        state.flag_handle_key.clone()
    }
}
