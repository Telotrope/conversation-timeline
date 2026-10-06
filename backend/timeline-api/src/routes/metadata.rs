//! Conversation metadata (plan `docs/plans/2026-10-05-screen-flow.md` §8c):
//!
//! - `GET /uploads` lists your files, for the page's Files tab and its
//!   Describe page, built from the conversations' records.
//! - `PUT /uploads/{upload_id}/metadata` changes the details of every
//!   conversation that first came in that file.
//! - `PUT /conversations/{conversation_id}/metadata` changes one
//!   conversation's details, start and end included. A conversation with
//!   messages of unknown time is placed by its start and end, so changing
//!   them moves its session (plan
//!   `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4e).
//!
//! The first two answer in parts (§8c): each does as much as fits in the
//! request's time limit and answers with a cursor to carry on from. An edit
//! is the same every time it is sent, so repeating one is harmless.
//!
//! An edit changes only the fields it carries, and marks them confirmed
//! (`MetadataEdit`). Another user's file or conversation is answered 404,
//! like one that doesn't exist, so it reveals nothing.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use timeline_core::conversation_metadata::{
    ConversationMedium, ConversationSpan, MetadataEdit, MetadataOrigin, Participants,
};
use timeline_core::labels::FileName;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::StoreError;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::uploads::{UploadOutcome, UploadOutcomeStore};
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_session::{Placement, StoredSession};
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::request_record::note;
use crate::routes::conversations::PartQuery;

/// How often a record write is redone after someone else changed the record
/// first.
const REDOS: usize = 5;

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

#[derive(Serialize)]
pub struct UploadsPart {
    /// This part's files, newest first; the page sorts the whole list again.
    pub uploads: Vec<UploadListing>,
    /// Every file of the user's, for the page's bar.
    pub total: usize,
    pub cursor: Option<String>,
    pub data_version: u64,
}

/// `GET /uploads`: your files, in parts.
pub async fn list_uploads(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(summaries_store): State<Arc<dyn ConversationSummaryStore>>,
    State(outcomes): State<Arc<dyn UploadOutcomeStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<BudgetSetting>,
    Query(query): Query<PartQuery>,
) -> Result<Json<UploadsPart>, ApiError> {
    let after = parse_for(query.cursor.as_deref(), "GET /uploads", |c| match c {
        Cursor::Uploads { after } => Some(after),
        _ => None,
    })?;
    let record = user_records.get(&user_id).await?;
    let summaries = summaries_store.list_for_user(&user_id).await?;
    // Keyed by the id's text so the order of the reads, and so the cursor,
    // is stable.
    let mut upload_ids: BTreeMap<String, UploadId> = BTreeMap::new();
    for s in &summaries {
        upload_ids.insert(s.source.upload_id.to_string(), s.source.upload_id);
        for added in &s.additions {
            upload_ids.insert(added.to_string(), *added);
        }
    }
    let total = upload_ids.len();
    let mut budget = budget.start();
    let mut listings = Vec::new();
    let mut last = None;
    let mut finished = true;
    let start = after.map(|a| a.to_string());
    for (text, upload_id) in &upload_ids {
        if start.as_ref().is_some_and(|s| text <= s) {
            continue;
        }
        if !budget.take_step() {
            finished = false;
            break;
        }
        last = Some(*upload_id);
        if let Some(listing) =
            listing_for(outcomes.as_ref(), &user_id, *upload_id, &summaries).await?
        {
            listings.push(listing);
        }
    }
    listings.sort_by(|a, b| b.uploaded_at.cmp(&a.uploaded_at));
    note("uploads", listings.len());
    Ok(Json(UploadsPart {
        uploads: listings,
        total,
        cursor: match (finished, last) {
            (false, Some(after)) => Some(Cursor::Uploads { after }.to_text()),
            _ => None,
        },
        data_version: record.data_version,
    }))
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

/// The body of `PUT /uploads/{upload_id}/metadata`: the edit, and where an
/// earlier part stopped.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadEditRequest {
    #[serde(default)]
    pub participants: Option<Participants>,
    #[serde(default)]
    pub medium: Option<ConversationMedium>,
    #[serde(default)]
    pub span: Option<ConversationSpan>,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct UploadEditPart {
    /// The records this part changed.
    pub conversations: Vec<ConversationSummary>,
    /// Conversations changed so far, of the file's total.
    pub done: usize,
    pub total: usize,
    pub cursor: Option<String>,
    pub data_version: u64,
}

/// Applies `edit` to the stored record of `conversation_id`, read afresh;
/// redone when someone else changed the record first.
async fn apply_edit(
    store: &dyn ConversationSummaryStore,
    user_id: &UserId,
    conversation_id: ConversationId,
    edit: &MetadataEdit,
) -> Result<ConversationSummary, ApiError> {
    for _ in 0..REDOS {
        let mut summary = store
            .get(user_id, conversation_id)
            .await?
            .ok_or(ApiError::NotFound)?;
        edit.apply_to(&mut summary);
        match store.put(user_id, summary).await {
            Ok(stored) => return Ok(stored),
            Err(StoreError::Conflict) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(ApiError::Store(StoreError::Conflict))
}

/// `PUT /uploads/{upload_id}/metadata`: the edit, applied to every
/// conversation that first came in this file, in parts. Start and end differ
/// by conversation, so an edit carrying them is refused. Answers with the
/// changed records.
#[allow(clippy::too_many_arguments)]
pub async fn edit_upload(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationSummaryStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<BudgetSetting>,
    Path(upload_id): Path<UploadId>,
    body: Result<Json<UploadEditRequest>, JsonRejection>,
) -> Result<Json<UploadEditPart>, ApiError> {
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let after = parse_for(
        request.cursor.as_deref(),
        "PUT /uploads/{id}/metadata",
        |c| match c {
            Cursor::UploadEdit { after } => Some(after),
            _ => None,
        },
    )?;
    let edit = MetadataEdit {
        participants: request.participants,
        medium: request.medium,
        span: request.span,
    };
    if edit == MetadataEdit::default() {
        return Err(ApiError::BadRequest(
            "nothing to save: give at least one of participants, medium, span".to_string(),
        ));
    }
    if edit.span.is_some() {
        return Err(ApiError::BadRequest(
            "a file's conversations each have their own start and end; change them one conversation at a time"
                .to_string(),
        ));
    }
    let mut ids: Vec<ConversationId> = store
        .list_for_user(&user_id)
        .await?
        .into_iter()
        .filter(|s| s.source.upload_id == upload_id)
        .map(|s| s.conversation_id)
        .collect();
    if ids.is_empty() {
        return Err(ApiError::NotFound);
    }
    ids.sort();
    let total = ids.len();
    let mut budget = budget.start();
    let mut changed = Vec::new();
    let mut last = None;
    let mut finished = true;
    for id in ids.iter().filter(|id| after.is_none_or(|a| **id > a)) {
        if !budget.take_step() {
            finished = false;
            break;
        }
        changed.push(apply_edit(store.as_ref(), &user_id, *id, &edit).await?);
        last = Some(*id);
    }
    let done = match last {
        Some(last) => ids.iter().filter(|id| **id <= last).count(),
        None => after.map_or(0, |a| ids.iter().filter(|id| **id <= a).count()),
    };
    note("conversations_changed", changed.len());
    // A file's details change no messages, flags or sessions, so the data
    // version stays.
    let record = user_records.get(&user_id).await?;
    Ok(Json(UploadEditPart {
        conversations: changed,
        done,
        total,
        cursor: match (finished, last) {
            (false, Some(after)) => Some(Cursor::UploadEdit { after }.to_text()),
            _ => None,
        },
        data_version: record.data_version,
    }))
}

/// `PUT /conversations/{conversation_id}/metadata`: the edit, applied to
/// one conversation. Answers with its changed record. A conversation
/// placed by its start and end (§4e) has its session moved with them.
pub async fn edit_conversation(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationSummaryStore>>,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    Path(conversation_id): Path<ConversationId>,
    body: Result<Json<MetadataEdit>, JsonRejection>,
) -> Result<Json<ConversationSummary>, ApiError> {
    let edit = read_edit(body)?;
    let summary = apply_edit(store.as_ref(), &user_id, conversation_id, &edit).await?;
    if edit.span.is_some() {
        let moved: Vec<StoredSession> = sessions
            .sessions_of(&user_id, conversation_id)
            .await?
            .into_iter()
            .filter(|s| s.placement == Placement::Span)
            .map(|s| StoredSession {
                start: summary.span.start().with_timezone(&Utc),
                end: summary.span.end().with_timezone(&Utc),
                ..s
            })
            .collect();
        if !moved.is_empty() {
            sessions.put_sessions(&user_id, &moved).await?;
            // The sessions the page draws changed.
            user_records.raise_version(&user_id).await?;
        }
    }
    Ok(Json(summary))
}
