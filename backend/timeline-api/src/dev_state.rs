//! State for the `_dev`-namespaced, local-testing-only routes. Deliberately
//! separate from [`crate::state::AppState`] -- this is the one place
//! `AutoFlagWriter` and `UploadOutcomeStore` are reachable in this binary
//! (the local-dev PUT handler plays the part of the real S3-triggered
//! processing Lambda -- see `routes::dev_local_storage`'s module doc), and
//! per the migration plan's §V2a neither must ever be reachable from a
//! user-facing route. This whole router must never be merged in when
//! running under Lambda (see `main.rs`).

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::message_flags::AutoFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadOutcomeStore;

#[derive(Clone)]
pub struct DevState {
    pub object_store: Arc<dyn ObjectStore>,
    pub upload_outcome_store: Arc<dyn UploadOutcomeStore>,
    pub conversation_summary_store: Arc<dyn ConversationSummaryStore>,
    pub auto_flag_writer: Arc<dyn AutoFlagWriter>,
}

impl FromRef<DevState> for Arc<dyn ObjectStore> {
    fn from_ref(state: &DevState) -> Self {
        state.object_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn UploadOutcomeStore> {
    fn from_ref(state: &DevState) -> Self {
        state.upload_outcome_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn ConversationSummaryStore> {
    fn from_ref(state: &DevState) -> Self {
        state.conversation_summary_store.clone()
    }
}

impl FromRef<DevState> for Arc<dyn AutoFlagWriter> {
    fn from_ref(state: &DevState) -> Self {
        state.auto_flag_writer.clone()
    }
}
