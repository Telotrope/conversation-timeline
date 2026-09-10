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
use timeline_core::ports::conversations::ConversationStore;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::message_flags::AutoFlagWriter;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadStore;

use crate::error::ApiError;
use crate::processing::process_upload;

/// `raw/{user_id}/{upload_id}.json` is the only key shape
/// [`crate::routes::uploads::create_upload`] ever generates, so it's the
/// only shape this parses. Anything else (an export key, a malformed key)
/// is `None` -- the PUT still stores the bytes, it just doesn't trigger
/// processing, matching production where only a raw-prefix upload fires
/// the S3 event in the first place.
fn parse_raw_upload_key(key: &str) -> Option<(UserId, UploadId)> {
    let rest = key.strip_prefix("raw/")?;
    let (user_part, upload_part) = rest.split_once('/')?;
    if user_part.is_empty() {
        return None;
    }
    let upload_id_str = upload_part.strip_suffix(".json")?;
    let upload_id = UploadId(upload_id_str.parse().ok()?);
    Some((UserId(user_part.to_string()), upload_id))
}

pub async fn put_object(
    Path(key): Path<String>,
    State(object_store): State<Arc<dyn ObjectStore>>,
    State(upload_store): State<Arc<dyn UploadStore>>,
    State(conversation_store): State<Arc<dyn ConversationStore>>,
    State(auto_flag_writer): State<Arc<dyn AutoFlagWriter>>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    object_store.put(&key, body.to_vec()).await?;

    if let Some((user_id, upload_id)) = parse_raw_upload_key(&key) {
        // Local substitute for the real S3 ObjectCreated event -- see
        // module doc. A real deployment logs and moves on if processing
        // fails (the upload is left in a `Failed` state for the client to
        // see); this does the same rather than turning a processing bug
        // into a 500 on what is, from the client's point of view, just the
        // file upload succeeding.
        if let Err(e) = process_upload(
            object_store.as_ref(),
            upload_store.as_ref(),
            conversation_store.as_ref(),
            auto_flag_writer.as_ref(),
            &user_id,
            upload_id,
        )
        .await
        {
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
