//! State for the `_dev`-namespaced, local-testing-only routes. Deliberately
//! separate from [`crate::state::AppState`] -- this is the one place
//! upload processing's stores, the `MessageRowWriter` among them, are
//! reachable in this binary (the local-dev PUT handler plays the part of the
//! real S3-triggered processing Lambda -- see `routes::dev_local_storage`'s
//! module doc), and per the migration plan's §V2a none of that must ever be
//! reachable from a user-facing route. This whole router must never be
//! merged in when running under Lambda (see `main.rs`).

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_core::ports::object_store::ObjectStore;
use timeline_storage::memory::resettable::Resettable;

use crate::s3_trigger::ProcessingStores;

#[derive(Clone)]
pub struct DevState {
    /// What the local upload route processes an upload with.
    pub processing: ProcessingStores,
    /// Every store `POST /_dev/reset` should empty. Held separately from the
    /// port handles above because emptying a store is not a storage-port
    /// capability -- see `timeline_storage::memory::resettable`. The same
    /// underlying object typically appears both here and as one of the ports.
    pub resettable: Arc<Vec<Arc<dyn Resettable>>>,
}

impl FromRef<DevState> for Arc<Vec<Arc<dyn Resettable>>> {
    fn from_ref(state: &DevState) -> Self {
        state.resettable.clone()
    }
}

impl FromRef<DevState> for ProcessingStores {
    fn from_ref(state: &DevState) -> Self {
        state.processing.clone()
    }
}

impl FromRef<DevState> for Arc<dyn ObjectStore> {
    fn from_ref(state: &DevState) -> Self {
        state.processing.object_store.clone()
    }
}
