//! `GET /sessions`: every session of the user's, with its counts (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5, §6), in
//! parts (§8c). The Calendar, the Conversations tab and three analyses are
//! drawn from these and the conversation records alone.

use std::sync::Arc;

use axum::extract::{Query, State};
use axum::Json;
use serde::Serialize;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_session::StoredSession;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::request_record::note;
use crate::routes::conversations::PartQuery;

/// Sessions read from storage at a time.
const PAGE: usize = 200;

/// The most sessions one part holds, about 1 MB: the time limit alone
/// wouldn't keep a part under Lambda's 6 MB limit on an answer for a user
/// with very many sessions.
pub const MAX_PER_PART: usize = 2_000;

#[derive(Serialize)]
pub struct SessionsPart {
    pub sessions: Vec<StoredSession>,
    /// Every session of the user's, for the page's bar.
    pub total: usize,
    pub cursor: Option<String>,
    pub data_version: u64,
}

pub async fn list_sessions(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn SessionStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<timeline_core::work_budget::BudgetSetting>,
    Query(query): Query<PartQuery>,
) -> Result<Json<SessionsPart>, ApiError> {
    let mut after = parse_for(query.cursor.as_deref(), "GET /sessions", |c| match c {
        Cursor::Sessions { after } => Some(after),
        _ => None,
    })?;
    let record = user_records.get(&user_id).await?;
    let mut budget = BudgetSetting::start(budget);
    let mut sessions = Vec::new();
    let finished = 'reading: loop {
        let page = store.sessions_page(&user_id, after, PAGE).await?;
        let full = page.len() == PAGE;
        for session in page {
            if sessions.len() >= MAX_PER_PART || !budget.take_step() {
                break 'reading false;
            }
            after = Some(session.key());
            sessions.push(session);
        }
        if !full {
            break true;
        }
    };
    note("sessions", sessions.len());
    Ok(Json(SessionsPart {
        sessions,
        total: record.totals.sessions,
        cursor: match (finished, after) {
            (false, Some(after)) => Some(Cursor::Sessions { after }.to_text()),
            _ => None,
        },
        data_version: record.data_version,
    }))
}
