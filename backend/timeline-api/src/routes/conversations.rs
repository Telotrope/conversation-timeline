//! `GET /conversations` and `GET /conversations/{id}/files` (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5, §8c).
//!
//! Both answer in parts: as much as fits in the request's time limit, then
//! a cursor to carry on from. Every answer carries the user's data version,
//! so the page can tell the data changed between parts.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use timeline_core::message_filter::MessageFilter;
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::messages::MessageReader;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_message::{Entry, FileRef};
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::message_query::{find_messages, EntryVisitor, Flow, WalkOrder, WalkStores};
use crate::request_record::note;

/// Records read from storage at a time.
const PAGE: usize = 100;

#[derive(Deserialize)]
pub struct PartQuery {
    pub cursor: Option<String>,
}

#[derive(Serialize)]
pub struct ConversationsPart {
    pub conversations: Vec<ConversationSummary>,
    /// Every conversation of the user's, for the page's bar.
    pub total: usize,
    /// Where to carry on; `null` in the last part.
    pub cursor: Option<String>,
    pub data_version: u64,
}

pub async fn list_conversations(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(store): State<Arc<dyn ConversationSummaryStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<BudgetSetting>,
    Query(query): Query<PartQuery>,
) -> Result<Json<ConversationsPart>, ApiError> {
    let mut after = parse_for(query.cursor.as_deref(), "GET /conversations", |c| match c {
        Cursor::Conversations { after } => Some(after),
        _ => None,
    })?;
    let record = user_records.get(&user_id).await?;
    let mut budget = budget.start();
    let mut conversations = Vec::new();
    let finished = 'reading: loop {
        let page = store.list_page(&user_id, after, PAGE).await?;
        let full = page.len() == PAGE;
        for summary in page {
            if !budget.take_step() {
                break 'reading false;
            }
            after = Some(summary.conversation_id);
            conversations.push(summary);
        }
        if !full {
            break true;
        }
    };
    note("conversations", conversations.len());
    Ok(Json(ConversationsPart {
        conversations,
        total: record.totals.conversations,
        cursor: match (finished, after) {
            (false, Some(after)) => Some(Cursor::Conversations { after }.to_text()),
            _ => None,
        },
        data_version: record.data_version,
    }))
}

/// One file a conversation's message carries, for the Conversations tab's
/// list (§4).
#[derive(Serialize)]
pub struct ListedFile {
    pub message_id: MessageId,
    /// When the message was sent; `null` when unknown (§4e).
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    pub sender: Sender,
    #[serde(flatten)]
    pub file: FileRef,
}

#[derive(Serialize)]
pub struct FilesPart {
    pub files: Vec<ListedFile>,
    pub sessions_done: usize,
    pub sessions_total: usize,
    pub cursor: Option<String>,
    pub data_version: u64,
}

struct CollectFiles(Vec<ListedFile>);

#[async_trait]
impl EntryVisitor for CollectFiles {
    async fn entry(
        &mut self,
        entry: &Entry,
        _session: &StoredSession,
        _before: WalkCursor,
    ) -> Result<Flow, ApiError> {
        if let Entry::Message(message) = entry {
            for file in message.files() {
                self.0.push(ListedFile {
                    message_id: message.key.id,
                    at: message.key.time().known(),
                    sender: message.sender.clone(),
                    file: file.clone(),
                });
            }
        }
        Ok(Flow::Continue)
    }
}

/// `GET /conversations/{id}/files`: every file a conversation's messages
/// presented or carry, in time order.
#[allow(clippy::too_many_arguments)]
pub async fn list_files(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(messages): State<Arc<dyn MessageReader>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(budget): State<BudgetSetting>,
    Path(conversation_id): Path<ConversationId>,
    Query(query): Query<PartQuery>,
) -> Result<Json<FilesPart>, ApiError> {
    let start = parse_for(
        query.cursor.as_deref(),
        "GET /conversations/{id}/files",
        |c| match c {
            Cursor::Files { walk } => Some(walk),
            _ => None,
        },
    )?;
    let record = user_records.get(&user_id).await?;
    let mut collect = CollectFiles(Vec::new());
    let end = find_messages(
        WalkStores {
            sessions: sessions.as_ref(),
            messages: messages.as_ref(),
        },
        &user_id,
        &MessageFilter::conversation(conversation_id),
        &WalkOrder::Key,
        start,
        &mut budget.start(),
        &mut collect,
    )
    .await?;
    Ok(Json(FilesPart {
        files: collect.0,
        sessions_done: end.sessions_done,
        sessions_total: end.sessions_total,
        cursor: end.cursor.map(|walk| Cursor::Files { walk }.to_text()),
        data_version: record.data_version,
    }))
}
