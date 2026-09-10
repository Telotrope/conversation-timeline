//! State for the `_dev`-namespaced, local-testing-only routes. Deliberately
//! separate from [`crate::state::AppState`] -- this is the one place
//! `AutoFlagWriter` is reachable in this binary, and per the migration
//! plan's §V2a it must never be reachable from a user-facing route, and
//! this whole router must never be merged in when running under Lambda
//! (see `main.rs`).

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_core::ports::conversations::ConversationStore;
use timeline_core::ports::message_flags::AutoFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadStore;

#[derive(Clone)]
pub struct DevState {
    pub object_store: Arc<dyn ObjectStore>,
    pub upload_store: Arc<dyn UploadStore>,
    pub conversation_store: Arc<dyn ConversationStore>,
    pub auto_flag_writer: Arc<dyn AutoFlagWriter>,
}

impl FromRef<DevState> for Arc<dyn ObjectStore> {
    fn from_ref(state: &DevState) -> Self {
        state.object_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn UploadStore> {
    fn from_ref(state: &DevState) -> Self {
        state.upload_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn ConversationStore> {
    fn from_ref(state: &DevState) -> Self {
        state.conversation_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn AutoFlagWriter> {
    fn from_ref(state: &DevState) -> Self {
        state.auto_flag_writer.clone()
    }
}
