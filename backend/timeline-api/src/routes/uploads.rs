//! `POST /uploads` -- issues a presigned S3 PUT URL, per the migration plan
//! section 1.6. The client then `PUT`s its `conversations.json` straight to
//! S3, never through this Lambda -- that's what keeps a 60MB export well
//! clear of API Gateway's 10MB synchronous payload limit. Nothing is
//! written to any storage port here: per the migration plan's
//! §V2a-revision, the raw object's key is a pure function of
//! `(user_id, upload_id)` (see
//! [`timeline_core::ports::uploads::raw_object_key`]), so there is nothing
//! to persist before the client's PUT lands.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use timeline_core::ports::ids::UploadId;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::raw_object_key;

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;

/// How long the presigned upload URL stays valid -- long enough for a slow
/// connection to push a large export, short enough not to be a
/// long-lived credential leak if the URL is ever logged somewhere it
/// shouldn't be.
const UPLOAD_URL_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Serialize)]
pub struct CreateUploadResponse {
    pub upload_id: UploadId,
    pub upload_url: String,
}

pub async fn create_upload(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(object_store): State<Arc<dyn ObjectStore>>,
) -> Result<Json<CreateUploadResponse>, ApiError> {
    let upload_id = UploadId(uuid::Uuid::new_v4());
    let key = raw_object_key(&user_id, upload_id);

    let upload_url = object_store.presign_put(&key, UPLOAD_URL_TTL).await?;

    Ok(Json(CreateUploadResponse {
        upload_id,
        upload_url,
    }))
}
