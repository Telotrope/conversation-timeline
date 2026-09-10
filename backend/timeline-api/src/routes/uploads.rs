//! `POST /uploads` -- issues a presigned S3 PUT URL and records the upload
//! as pending, per the migration plan section 1.6. The client then `PUT`s
//! its `conversations.json` straight to S3, never through this Lambda --
//! that's what keeps a 60MB export well clear of API Gateway's 10MB
//! synchronous payload limit.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::Json;
use serde::Serialize;
use timeline_core::ports::ids::UploadId;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::UploadStore;

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
    State(upload_store): State<Arc<dyn UploadStore>>,
) -> Result<Json<CreateUploadResponse>, ApiError> {
    let upload_id = UploadId(uuid::Uuid::new_v4());
    let raw_object_key = format!("raw/{user_id}/{upload_id}.json");

    let upload_url = object_store
        .presign_put(&raw_object_key, UPLOAD_URL_TTL)
        .await?;
    upload_store
        .create_pending(&user_id, upload_id, &raw_object_key)
        .await?;

    Ok(Json(CreateUploadResponse {
        upload_id,
        upload_url,
    }))
}
