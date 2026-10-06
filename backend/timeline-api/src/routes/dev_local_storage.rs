//! `_dev`-only routes standing in for two real-S3-specific mechanisms so a
//! real browser can drive the in-memory adapters end to end without AWS or
//! a container: a presigned URL is normally something the client `PUT`s to
//! directly, and an upload landing in S3 normally fires an event that
//! triggers a separate processing Lambda. Locally, this one endpoint plays
//! both parts -- see the migration plan's §V2a for the full design and
//! why. **Never mounted when running under Lambda** (see `main.rs`).

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::parse_raw_object_key;

use crate::error::ApiError;
use crate::processing::process_upload;
use crate::s3_trigger::ProcessingStores;

pub async fn put_object(
    Path(key): Path<String>,
    State(stores): State<ProcessingStores>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    stores.object_store.put(&key, body.to_vec()).await?;

    // Only a raw upload's key triggers processing, matching production,
    // where only keys under `raw/` fire the S3 event. Anything else (an
    // export, a malformed key) is just stored.
    if let Some((user_id, upload_id)) = parse_raw_object_key(&key) {
        // Local substitute for the real S3 ObjectCreated event -- see
        // module doc. A real deployment logs and moves on if processing
        // fails (the upload is left in a `Failed` state for the client to
        // see); this does the same rather than turning a processing bug
        // into a 500 on what is, from the client's point of view, just the
        // file upload succeeding.
        if let Err(e) = process_upload(&stores, &user_id, upload_id).await {
            eprintln!("local-dev upload processing failed for {user_id}/{upload_id}: {e}");
        }
    }

    Ok(StatusCode::OK)
}

pub async fn get_object(
    Path(key): Path<String>,
    State(object_store): State<Arc<dyn ObjectStore>>,
) -> Result<Bytes, ApiError> {
    let data = object_store.get(&key).await?;
    Ok(Bytes::from(data))
}
