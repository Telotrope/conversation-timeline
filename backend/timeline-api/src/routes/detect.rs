//! `POST /detect` -- the scan: runs the non-generative (keyword, dictionary
//! and sentiment) flag detection over your messages (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8).
//!
//! Detection deliberately does **not** happen at upload time: it runs only
//! when the user asks for it, and reports progress while it does (see
//! `docs/plans/completed/2026-09-28-frontend-quality-of-life.md` Phase 4).
//!
//! **In parts.** The scan reads your message rows through the shared
//! `find_messages`, within the request's time limit (§8c), checking the
//! clock before every message, so one long conversation can't make a
//! request run past it; the cursor can stop inside a session. Each answer
//! says how many sessions are done of how many, so the page's bar shows a
//! real percentage, and the page asks again with the cursor until there is
//! none. Each session the scan finishes is recounted (§6), and every part
//! that scanned a message raises the user's data version.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use timeline_core::flag_view::FlagView;
use timeline_core::message_filter::MessageFilter;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{AutoFlagWriter, MessageReader};
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_message::Entry;
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::message_query::{find_messages, EntryVisitor, Flow, WalkOrder, WalkStores};
use crate::processing::heuristic_flags;
use crate::request_record::note;
use crate::session_recount::recount_session;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct DetectRequest {
    /// Where the last part stopped; absent to start.
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct DetectResponse {
    pub sessions_done: usize,
    pub sessions_total: usize,
    /// Your messages scanned by this part.
    pub messages_detected: usize,
    /// Where to carry on, or `null` once the scan is complete.
    pub cursor: Option<String>,
    pub data_version: u64,
}

struct Scan<'a> {
    user_id: &'a UserId,
    writer: &'a dyn AutoFlagWriter,
    stores: WalkStores<'a>,
    session_store: &'a dyn SessionStore,
    scanned: usize,
}

#[async_trait]
impl EntryVisitor for Scan<'_> {
    async fn entry(
        &mut self,
        entry: &Entry,
        _session: &StoredSession,
        _before: WalkCursor,
    ) -> Result<Flow, ApiError> {
        if let Entry::Message(message) = entry {
            let flags = heuristic_flags(&message.text());
            self.writer
                .set_auto_flags(self.user_id, message.key, flags)
                .await?;
            self.scanned += 1;
        }
        Ok(Flow::Continue)
    }

    async fn group_done(&mut self, group: &[StoredSession]) -> Result<(), ApiError> {
        for session in group {
            recount_session(self.stores, self.session_store, self.user_id, session).await?;
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn detect(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(messages): State<Arc<dyn MessageReader>>,
    State(auto_flag_writer): State<Arc<dyn AutoFlagWriter>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<BudgetSetting>,
    body: Result<Json<DetectRequest>, JsonRejection>,
) -> Result<Json<DetectResponse>, ApiError> {
    let Json(request) = body.map_err(|rejection| ApiError::BadRequest(rejection.body_text()))?;
    let start = parse_for(request.cursor.as_deref(), "POST /detect", |c| match c {
        Cursor::Scan { walk } => Some(walk),
        _ => None,
    })?;
    let stores = WalkStores {
        sessions: sessions.as_ref(),
        messages: messages.as_ref(),
    };
    let mut scan = Scan {
        user_id: &user_id,
        writer: auto_flag_writer.as_ref(),
        stores,
        session_store: sessions.as_ref(),
        scanned: 0,
    };
    let end = find_messages(
        stores,
        &user_id,
        &MessageFilter::your_messages(FlagView::Both),
        &WalkOrder::Key,
        start,
        &mut budget.start(),
        &mut scan,
    )
    .await?;
    let record = if scan.scanned > 0 {
        user_records.raise_version(&user_id).await?
    } else {
        user_records.get(&user_id).await?
    };
    // For this request's log line (crate::request_log).
    note("resumed", start.is_some());
    note("sessions_done", end.sessions_done);
    note("sessions_total", end.sessions_total);
    note("messages_detected", scan.scanned);
    Ok(Json(DetectResponse {
        sessions_done: end.sessions_done,
        sessions_total: end.sessions_total,
        messages_detected: scan.scanned,
        cursor: end.cursor.map(|walk| Cursor::Scan { walk }.to_text()),
        data_version: record.data_version,
    }))
}
