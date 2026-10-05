//! Conversation metadata (plan `docs/plans/2026-10-05-screen-flow.md` §8c):
//!
//! - `GET /uploads` lists your files, for the page's Files tab and its
//!   Describe page, built from the conversations' records.
//! - `PUT /uploads/{upload_id}/metadata` changes the details of every
//!   conversation that first came in that file.
//! - `PUT /conversations/{conversation_id}/metadata` changes one
//!   conversation's details, start and end included.
//!
//! An edit changes only the fields it carries, and marks them confirmed
//! (`MetadataEdit`). Another user's file or conversation is answered 404,
//! like one that doesn't exist, so it reveals nothing.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::Serialize;
use timeline_core::conversation_metadata::{
    ConversationMedium, MetadataEdit, MetadataOrigin, Participants,
};
use timeline_core::labels::FileName;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::request_record::note;

/// One file in `GET /uploads`. A field that differs between the file's
/// conversations (some were edited one by one) is `null`: the page shows it
/// as "varies".
#[derive(Serialize)]
pub struct UploadListing {
    pub upload_id: UploadId,
    pub file_name: FileName,
    pub uploaded_at: DateTime<Utc>,
    pub file_written_at: Option<DateTime<Utc>>,
    /// Conversations that first came in this file. Its details belong to them.
    pub conversation_count: usize,
    /// Conversations in this file that an earlier file had already brought.
    pub already_present: usize,
    /// Of those, the ones this file added messages to.
    pub gained_messages: usize,
    pub participants: Option<Participants>,
    pub medium: Option<ConversationMedium>,
    pub details_origin: Option<MetadataOrigin>,
}

/// The one value every item shares, or `None` when they differ or there
/// are none.
fn shared<T: Clone + PartialEq>(mut values: impl Iterator<Item = T>) -> Option<T> {
    let first = values.next()?;
    values.all(|v| v == first).then_some(first)
}

async fn listing_for(
    outcomes: &dyn UploadOutcomeStore,
    user_id: &UserId,
    upload_id: UploadId,
    summaries: &[ConversationSummary],
) -> Result<Option<UploadListing>, ApiError> {
    let own: Vec<&ConversationSummary> = summaries
        .iter()
        .filter(|s| s.source.upload_id == upload_id)
        .collect();
    let gained_messages = summaries
        .iter()
        .filter(|s| s.additions.contains(&upload_id))
        .count();
    let in_file = match outcomes.get_outcome(user_id, upload_id).await? {
        Some(UploadOutcome::Ready { conversation_ids }) => conversation_ids.len(),
        // Unreachable backstop: every file named in a record finished
        // processing as Ready before the record was written.
        _ => own.len(),
    };
    let (file_name, uploaded_at, file_written_at) = match own.first() {
        Some(s) => (
            s.source.file_name.clone(),
            s.source.uploaded_at,
            s.source.file_written_at,
        ),
        // A file that only added messages to conversations already present.
        None => match outcomes.get_received(user_id, upload_id).await? {
            Some(facts) => (facts.file_name, facts.uploaded_at, facts.file_written_at),
            // Unreachable backstop: processing refuses an upload with no
            // record, so a file named in a record always has one.
            None => return Ok(None),
        },
    };
    Ok(Some(UploadListing {
        upload_id,
        file_name,
        uploaded_at,
        file_written_at,
        conversation_count: own.len(),
        already_present: in_file.saturating_sub(own.len()),
        gained_messages,
        participants: shared(own.iter().map(|s| s.participants.clone())),
        medium: shared(own.iter().map(|s| s.medium.clone())),
        details_origin: shared(own.iter().map(|s| s.details_origin)),
    }))
}

/// `GET /uploads`: your files, newest first.
pub async fn list_uploads(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(summaries_store): State<Arc<dyn ConversationSummaryStore>>,
    State(outcomes): State<Arc<dyn UploadOutcomeStore>>,
) -> Result<Json<Vec<UploadListing>>, ApiError> {
    let summaries = summaries_store.list_for_user(&user_id).await?;
    // Keyed by the id's text so the order of the reads is stable.
    let mut upload_ids: BTreeMap<String, UploadId> = BTreeMap::new();
    for s in &summaries {
        upload_ids.insert(s.source.upload_id.to_string(), s.source.upload_id);
        for added in &s.additions {
            upload_ids.insert(added.to_string(), *added);
        }
    }
    let mut listings = Vec::with_capacity(upload_ids.len());
    for upload_id in upload_ids.into_values() {
        if let Some(listing) =
            listing_for(outcomes.as_ref(), &user_id, upload_id, &summaries).await?
        {
            listings.push(listing);
        }
    }
    listings.sort_by(|a, b| b.uploaded_at.cmp(&a.uploaded_at));
    note("uploads", listings.len());
    Ok(Json(listings))
}

fn read_edit(body: Result<Json<MetadataEdit>, JsonRejection>) -> Result<MetadataEdit, ApiError> {
    // axum would answer a malformed body with its own 422; these routes
    // answer 400 with serde's explanation, as the flag route does.
    let Json(edit) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    if edit == MetadataEdit::default() {
        return Err(ApiError::BadRequest(
            "nothing to save: give at least one of participants, medium, span".to_string(),
        ));
    }
    Ok(edit)
}

/// `PUT /uploads/{upload_id}/metadata`: the edit, applied to every
/// conversation that first came in this file. Start and end differ by
/// conversation, so an edit carrying them is refused. Answers with the
/// changed records.
pub async fn edit_upload(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationSummaryStore>>,
    Path(upload_id): Path<UploadId>,
    body: Result<Json<MetadataEdit>, JsonRejection>,
) -> Result<Json<Vec<ConversationSummary>>, ApiError> {
    let edit = read_edit(body)?;
    if edit.span.is_some() {
        return Err(ApiError::BadRequest(
            "a file's conversations each have their own start and end; change them one conversation at a time"
                .to_string(),
        ));
    }
    let mut changed: Vec<ConversationSummary> = store
        .list_for_user(&user_id)
        .await?
        .into_iter()
        .filter(|s| s.source.upload_id == upload_id)
        .collect();
    if changed.is_empty() {
        return Err(ApiError::NotFound);
    }
    for summary in &mut changed {
        edit.apply_to(summary);
        store.put(&user_id, summary.clone()).await?;
    }
    note("conversations_changed", changed.len());
    Ok(Json(changed))
}

/// `PUT /conversations/{conversation_id}/metadata`: the edit, applied to
/// one conversation. Answers with its changed record.
pub async fn edit_conversation(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationSummaryStore>>,
    Path(conversation_id): Path<ConversationId>,
    body: Result<Json<MetadataEdit>, JsonRejection>,
) -> Result<Json<ConversationSummary>, ApiError> {
    let edit = read_edit(body)?;
    let mut summary = store
        .get(&user_id, conversation_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    edit.apply_to(&mut summary);
    store.put(&user_id, summary.clone()).await?;
    Ok(Json(summary))
}
