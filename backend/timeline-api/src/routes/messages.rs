//! `GET /messages`: Review's rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b, §8b, §8c).
//!
//! The page sends its filters (a conversation, a time span and whether it
//! is a Calendar day, the flag menu, a search, the show switches) and where
//! to carry on. The server walks the matching messages in order (by time;
//! for a Calendar day by conversation name, then time) through the shared
//! [`find_messages`], and answers with:
//! - up to `rows` full rows from the cursor, each your message with its
//!   flags, its flag handle and, with `replies` on, the reply that follows
//!   it; and branch notes in their place (only with the flag menu on "All"
//!   and no search);
//! - how many rows matched up to where it stopped (`matched`, counting on
//!   from the `matched` the page sent);
//! - a cursor for every page of 50 rows that begins in this part, so the
//!   page can later ask for page 7 from page 7's cursor without ever holding
//!   every match;
//! - the cursor to carry on from, `null` once the walk is done.
//!
//! With `until=rows` the walk stops as soon as it has its rows (paging to
//! a page whose start is known); with `until=end` (the default) it goes on
//! counting matches until the time limit or the end, so the page can say
//! how many pages there are.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use timeline_core::flag_values::{FlagKind, MessageFlags};
use timeline_core::flag_view::FlagView;
use timeline_core::message_filter::{
    FlagFilter, MessageFilter, SearchText, SpanFilter, SpanKind, TimeSpan,
};
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::MessageReader;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_message::{BranchNote, Entry, FileRef, Piece};
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::flag_handles::{FlagHandle, FlagHandleKey};
use crate::message_query::{find_messages, EntryVisitor, Flow, WalkOrder, WalkStores};
use crate::request_record::note;

/// Rows per page of Review.
pub const PAGE_ROWS: usize = 50;

/// When the walk stops besides the time limit and the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Until {
    /// Go on counting matches after the rows are full.
    #[default]
    End,
    /// Stop once the rows are full.
    Rows,
}

/// The query string, as sent. Parsed into typed values by [`Filters::parse`].
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessagesQuery {
    pub conversation: Option<ConversationId>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// `range` (a session or an analysis point; the default) or `day`.
    pub span: Option<SpanKind>,
    pub flag: Option<String>,
    pub search: Option<String>,
    pub view: Option<FlagView>,
    #[serde(default)]
    pub replies: bool,
    pub cursor: Option<String>,
    /// Rows matched before the cursor, as the last part said.
    #[serde(default)]
    pub matched: usize,
    /// Of those, the notes.
    #[serde(default)]
    pub notes: usize,
    pub rows: Option<usize>,
    #[serde(default)]
    pub until: Until,
}

/// The page's flag menu, by the names its `<select>` uses.
fn flag_filter(raw: Option<&str>) -> Result<FlagFilter, ApiError> {
    Ok(match raw.unwrap_or("all") {
        "all" => FlagFilter::All,
        "flagged" => FlagFilter::Flagged,
        "caps" => FlagFilter::Only(FlagKind::Caps),
        "angry" => FlagFilter::Only(FlagKind::Angry),
        "critical" => FlagFilter::Only(FlagKind::Critical),
        "overridden" => FlagFilter::Overridden,
        other => {
            return Err(ApiError::BadRequest(format!(
                "flag must be one of all, flagged, caps, angry, critical, overridden, not {:?}",
                other.chars().take(40).collect::<String>()
            )))
        }
    })
}

impl MessagesQuery {
    fn filter(&self) -> Result<MessageFilter, ApiError> {
        let span = match (self.from, self.to) {
            (None, None) => None,
            (Some(from), Some(to)) => Some(SpanFilter {
                span: TimeSpan::new(from, to).map_err(|e| ApiError::BadRequest(e.to_string()))?,
                kind: self.span.unwrap_or(SpanKind::Range),
            }),
            _ => {
                return Err(ApiError::BadRequest(
                    "a time span needs both from and to".to_string(),
                ))
            }
        };
        Ok(MessageFilter {
            conversation: self.conversation,
            span,
            flag: flag_filter(self.flag.as_deref())?,
            search: self.search.as_deref().and_then(SearchText::parse),
            view: self.view.unwrap_or(FlagView::Both),
            every_entry: false,
            notes: true,
        })
    }
}

/// One of your messages as Review shows it.
#[derive(Debug, Serialize)]
pub struct MessageRow {
    pub conversation_id: ConversationId,
    pub message_id: MessageId,
    /// When it was sent; `null` when unknown (§4e).
    pub at: Option<DateTime<Utc>>,
    pub pieces: Vec<Piece>,
    pub attachments: Vec<FileRef>,
    pub flags: MessageFlags,
    /// Sent back with a flag save; proves the message is real.
    pub handle: FlagHandle,
    /// The reply that follows, with `replies` on and when there is one.
    pub reply: Option<Reply>,
}

/// Claude's reply to a message.
#[derive(Debug, Serialize)]
pub struct Reply {
    pub message_id: MessageId,
    pub pieces: Vec<Piece>,
}

/// One row of Review: your message, or a note where a branch was pruned.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Row {
    Message(MessageRow),
    Note(BranchNote),
}

/// Where a page of rows begins.
#[derive(Debug, Serialize)]
pub struct PageStart {
    /// From 0.
    pub page: usize,
    pub cursor: String,
}

#[derive(Debug, Serialize)]
pub struct MessagesPart {
    pub rows: Vec<Row>,
    /// Rows matched from the start of the results up to where this part
    /// stopped; notes included, so it counts rows for paging.
    pub matched: usize,
    /// Of those, the notes: the page's count of messages leaves them out.
    pub notes: usize,
    pub page_starts: Vec<PageStart>,
    pub cursor: Option<String>,
    pub sessions_done: usize,
    pub sessions_total: usize,
    pub data_version: u64,
}

/// Collects rows and page starts as the walk goes.
struct Collect<'a> {
    user_id: &'a UserId,
    key: &'a FlagHandleKey,
    wanted: usize,
    until: Until,
    matched: usize,
    rows: Vec<Row>,
    notes: usize,
    page_starts: Vec<PageStart>,
}

#[async_trait]
impl EntryVisitor for Collect<'_> {
    async fn entry(
        &mut self,
        entry: &Entry,
        _session: &StoredSession,
        before: WalkCursor,
    ) -> Result<Flow, ApiError> {
        if self.matched > 0 && self.matched.is_multiple_of(PAGE_ROWS) {
            self.page_starts.push(PageStart {
                page: self.matched / PAGE_ROWS,
                cursor: Cursor::Messages { walk: before }.to_text(),
            });
        }
        self.matched += 1;
        if self.rows.len() < self.wanted {
            self.rows.push(match entry {
                Entry::Note(note) => Row::Note(note.clone()),
                Entry::Message(m) => Row::Message(MessageRow {
                    conversation_id: m.key.conversation_id,
                    message_id: m.key.id,
                    at: m.key.time().known(),
                    pieces: m.pieces.clone(),
                    attachments: m.attachments.clone(),
                    flags: m.flags.unwrap_or_default(),
                    handle: self
                        .key
                        .handle_for(self.user_id, m.key.conversation_id, m.key.id),
                    reply: None,
                }),
            });
        }
        if matches!(entry, Entry::Note(_)) {
            self.notes += 1;
        }
        let full = self.rows.len() >= self.wanted;
        Ok(if full && self.until == Until::Rows {
            Flow::Stop
        } else {
            Flow::Continue
        })
    }
}

/// The reply that follows each message row: the next row of its
/// conversation, when it is one of Claude's messages.
async fn attach_replies(
    messages: &dyn MessageReader,
    user_id: &UserId,
    rows: &mut [Row],
) -> Result<(), ApiError> {
    for row in rows {
        let Row::Message(row) = row else { continue };
        let key = timeline_core::stored_message::EntryKey {
            conversation_id: row.conversation_id,
            at: row.at.unwrap_or(timeline_core::UNKNOWN_TIME),
            id: row.message_id,
        };
        if let Some(Entry::Message(next)) = messages.entry_after(user_id, key).await? {
            if next.sender == Sender::Assistant {
                row.reply = Some(Reply {
                    message_id: next.key.id,
                    pieces: next.pieces,
                });
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn list_messages(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(messages): State<Arc<dyn MessageReader>>,
    State(conversations): State<Arc<dyn ConversationSummaryStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(key): State<Arc<FlagHandleKey>>,
    State(budget): State<BudgetSetting>,
    query: Result<Query<MessagesQuery>, axum::extract::rejection::QueryRejection>,
) -> Result<Json<MessagesPart>, ApiError> {
    let Query(query) = query.map_err(|r| ApiError::BadRequest(r.body_text()))?;
    let filter = query.filter()?;
    let start = parse_for(query.cursor.as_deref(), "GET /messages", |c| match c {
        Cursor::Messages { walk } => Some(walk),
        _ => None,
    })?;
    let wanted = query.rows.unwrap_or(PAGE_ROWS).min(PAGE_ROWS);
    let order = match filter.span {
        Some(SpanFilter {
            kind: SpanKind::Day,
            ..
        }) => {
            let names: HashMap<ConversationId, String> = conversations
                .list_for_user(&user_id)
                .await?
                .into_iter()
                .map(|s| (s.conversation_id, s.name.0))
                .collect();
            WalkOrder::ConversationName(names)
        }
        _ => WalkOrder::Time,
    };
    let record = user_records.get(&user_id).await?;
    let mut collect = Collect {
        user_id: &user_id,
        key: &key,
        wanted,
        until: query.until,
        matched: query.matched,
        rows: Vec::new(),
        notes: query.notes,
        page_starts: Vec::new(),
    };
    let end = find_messages(
        WalkStores {
            sessions: sessions.as_ref(),
            messages: messages.as_ref(),
        },
        &user_id,
        &filter,
        &order,
        start,
        &mut budget.start(),
        &mut collect,
    )
    .await?;
    let mut rows = collect.rows;
    if query.replies {
        attach_replies(messages.as_ref(), &user_id, &mut rows).await?;
    }
    note("rows", rows.len());
    note("matched", collect.matched);
    Ok(Json(MessagesPart {
        rows,
        matched: collect.matched,
        notes: collect.notes,
        page_starts: collect.page_starts,
        cursor: end.cursor.map(|walk| Cursor::Messages { walk }.to_text()),
        sessions_done: end.sessions_done,
        sessions_total: end.sessions_total,
        data_version: record.data_version,
    }))
}
