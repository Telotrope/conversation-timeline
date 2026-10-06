//! `POST /uploads` -- issues a presigned S3 PUT URL, per the migration plan
//! section 1.6. The client then `PUT`s its `conversations.json` straight to
//! S3, never through this Lambda -- that's what keeps a 60MB export well
//! clear of API Gateway's 10MB synchronous payload limit. The raw object's
//! key is a pure function of `(user_id, upload_id)` (see
//! [`timeline_core::ports::uploads::raw_object_key`]), so the key itself is
//! never stored. What is stored, before the address is handed out, is what
//! the file itself can't say: its name, when it was last written, the
//! upload time and the name to give the human, which processing uses to
//! fill in each conversation's guessed metadata (plan
//! `docs/plans/2026-10-05-screen-flow.md` §8b-8c).
//!
//! `GET /uploads/{upload_id}` tells the page whether processing has
//! finished. On AWS, processing runs in a separate Lambda once the file lands
//! in S3, so the page asks until the answer is no longer `processing`
//! (migration plan §V2e, E3).

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use timeline_core::conversation_metadata::UploadFacts;
use timeline_core::labels::{FileName, PersonName};
use timeline_core::ports::ids::UploadId;
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::{
    raw_object_key, ProcessingProgress, UploadOutcome, UploadOutcomeStore,
};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::request_record::note;
use crate::s3_trigger::MAX_PROCESSING_ATTEMPTS;

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

/// The body of `POST /uploads`. The names are cleaned as they are read
/// (`timeline_core::labels`); an empty one is refused.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateUploadRequest {
    pub file_name: FileName,
    /// The browser's last-written time for the file, when it gives one.
    #[serde(default)]
    pub file_written_at: Option<DateTime<Utc>>,
    /// The signed-in account, which the guessed metadata names as the human.
    pub human_name: PersonName,
}

pub async fn create_upload(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(object_store): State<Arc<dyn ObjectStore>>,
    State(upload_outcome_store): State<Arc<dyn UploadOutcomeStore>>,
    body: Result<Json<CreateUploadRequest>, JsonRejection>,
) -> Result<Json<CreateUploadResponse>, ApiError> {
    // axum would answer a malformed body with its own 422; this route
    // answers 400 with serde's explanation, as the flag route does.
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let upload_id = UploadId(uuid::Uuid::new_v4());
    // For this request's log line (crate::request_log).
    note("upload_id", upload_id.0.to_string());
    let key = raw_object_key(&user_id, upload_id);

    upload_outcome_store
        .record_received(
            &user_id,
            upload_id,
            UploadFacts {
                file_name: request.file_name,
                uploaded_at: Utc::now(),
                file_written_at: request.file_written_at,
                human_name: request.human_name,
            },
        )
        .await?;

    let upload_url = object_store.presign_put(&key, UPLOAD_URL_TTL).await?;

    Ok(Json(CreateUploadResponse {
        upload_id,
        upload_url,
    }))
}

/// What `GET /uploads/{upload_id}` answers. `Processing` covers "no outcome
/// recorded yet", which is also what an upload id that doesn't exist, or
/// belongs to someone else, looks like: outcomes are stored under the
/// logged-in user's id, so another user's upload reveals nothing.
///
/// On AWS, `Processing` also says which attempt is running and why the
/// last one failed, so the page can show a retry rather than a silent wait
/// (plan `2026-10-02-upload-processing-failures.md` §3). Before the first
/// attempt, and always locally, those fields are absent and the answer is
/// plain `{"status": "processing"}`.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UploadStatusResponse {
    Processing {
        #[serde(skip_serializing_if = "Option::is_none")]
        attempt: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        max_attempts: Option<usize>,
        #[serde(skip_serializing_if = "Option::is_none")]
        last_error: Option<String>,
        /// How far the running attempt has got, written every second (plan
        /// 2026-10-06-load-only-what-the-page-shows.md §8b).
        #[serde(skip_serializing_if = "Option::is_none")]
        progress: Option<ProcessingProgress>,
    },
    Ready,
    Failed {
        reason: String,
    },
}

pub async fn upload_status(
    AuthenticatedUser(user_id): AuthenticatedUser,
    Path(upload_id): Path<String>,
    State(upload_outcome_store): State<Arc<dyn UploadOutcomeStore>>,
) -> Result<Json<UploadStatusResponse>, ApiError> {
    let upload_id = upload_id
        .parse()
        .map(UploadId)
        .map_err(|e| ApiError::BadRequest(format!("upload id is not a UUID: {e}")))?;
    let status = match upload_outcome_store
        .get_outcome(&user_id, upload_id)
        .await?
    {
        None => {
            let progress = upload_outcome_store
                .get_progress(&user_id, upload_id)
                .await?;
            let attempt = progress.as_ref().map(|p| p.attempts).filter(|&n| n > 0);
            UploadStatusResponse::Processing {
                attempt,
                max_attempts: attempt.map(|_| MAX_PROCESSING_ATTEMPTS),
                last_error: progress.as_ref().and_then(|p| p.last_error.clone()),
                progress: progress.and_then(|p| p.processing),
            }
        }
        Some(UploadOutcome::Ready { .. }) => UploadStatusResponse::Ready,
        Some(UploadOutcome::Failed { reason }) => UploadStatusResponse::Failed { reason },
    };
    Ok(Json(status))
}
