//! `GET /export` -- the annotated download, rebuilt from the stored rows
//! (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5, §8c).
//!
//! It holds every conversation's messages as text, with their citations,
//! your flags (`_claude_timeline_user`) and the automatic ones the scan
//! found (`_claude_timeline_auto`), and the names of their files. It no
//! longer reproduces the original export, since tool calls, tool results
//! and thinking are not kept, but it can be uploaded again: messages of text
//! alone are a valid export, and uploading it brings back your flags. Notes
//! for pruned branches are left out; the export format has nothing like
//! them. Every stored conversation has messages (processing drops empty
//! ones, plan §12.2), so walking sessions reaches every conversation. A
//! message of unknown time is written without `created_at`, so a
//! round trip keeps it unknown (§4e).
//!
//! **In parts.** The file is written in pieces, each reply carrying one as
//! text (`part`) with the flag handles of its messages; the page joins them
//! into the file and saves it when the last arrives. A part ends at the
//! request's time limit or once its text reaches [`PART_BYTES`] as sent
//! (well under Lambda's 6 MB limit on an answer), and the cursor says where
//! to carry on, including whether a conversation was left open.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::{Query, State};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Map, Value};
use timeline_core::message_filter::MessageFilter;
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::MessageReader;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::stored_message::{Entry, Piece, StoredMessage};
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::cursor::{parse_for, Cursor};
use crate::error::ApiError;
use crate::flag_handles::{FlagHandle, FlagHandleKey};
use crate::message_query::{find_messages, EntryVisitor, Flow, WalkOrder, WalkStores};
use crate::request_record::note;
use crate::routes::conversations::PartQuery;

/// The most a part's text takes in its reply, escaped as JSON.
pub const PART_BYTES: usize = 4 * 1024 * 1024;

/// The format version this server writes into the download; the page's
/// own downloads were version 2.
pub const FORMAT_VERSION: &str = "3";

#[derive(Serialize)]
pub struct ExportPart {
    /// The next piece of the file's text.
    pub part: String,
    /// One handle per message of yours in this part, by message id. Kept
    /// from the earlier export reply; `GET /messages` issues them too.
    pub flag_handles: HashMap<MessageId, FlagHandle>,
    pub sessions_done: usize,
    pub sessions_total: usize,
    /// Where to carry on; `null` with the file's last piece.
    pub cursor: Option<String>,
    pub data_version: u64,
}

/// One message as the export format writes it.
pub fn message_json(message: &StoredMessage) -> Value {
    let mut out = Map::new();
    out.insert("uuid".to_string(), json!(message.key.id));
    out.insert("sender".to_string(), json!(message.sender));
    if let Some(at) = message.key.time().known() {
        out.insert("created_at".to_string(), json!(at));
    }
    // Only a known parent is written. A message written with none counts as
    // unstated when the file is uploaded again, which leaves its
    // conversation whole rather than pruned: the download holds no
    // replaced branches.
    if let Some(parent) = message.parent {
        out.insert("parent_message_uuid".to_string(), json!(parent));
    }
    let content: Vec<Value> = message
        .pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Text { text, citations } => Some(json!({
                "type": "text",
                "text": text,
                "citations": citations.iter().map(|c| json!({
                    "start_index": c.start,
                    "end_index": c.end,
                    "details": {"url": match &c.address {
                        timeline_core::stored_message::CitedAddress::Web(a)
                        | timeline_core::stored_message::CitedAddress::Other(a) => a,
                    }},
                })).collect::<Vec<_>>(),
            })),
            Piece::File { .. } => None,
        })
        .collect();
    out.insert("content".to_string(), Value::Array(content));
    let files: Vec<Value> = message
        .files()
        .map(|f| json!({"file_name": f.name}))
        .collect();
    out.insert("files".to_string(), Value::Array(files));
    if let (Sender::Human, Some(flags)) = (&message.sender, message.flags) {
        if let Some(auto) = flags.auto {
            out.insert(
                "_claude_timeline_auto".to_string(),
                json!({"caps": auto.caps, "critical": auto.critical, "angry": auto.angry, "source": "heuristic"}),
            );
        }
        if flags.user.is_review() {
            out.insert("_claude_timeline_user".to_string(), json!(flags.user));
        }
    }
    Value::Object(out)
}

/// Writes the file's text as the walk goes.
struct Writer<'a> {
    user_id: &'a UserId,
    key: &'a FlagHandleKey,
    names: &'a HashMap<ConversationId, String>,
    text: String,
    /// The text's size once escaped as JSON in the reply.
    sent_bytes: usize,
    open: Option<ConversationId>,
    any: bool,
    handles: HashMap<MessageId, FlagHandle>,
    /// Conversations this part began writing, for the log line.
    begun: usize,
}

impl Writer<'_> {
    fn push(&mut self, piece: &str) {
        // Unreachable backstop: a string always serializes.
        self.sent_bytes += serde_json::to_string(piece)
            .expect("a string serializes")
            .len()
            - 2;
        self.text.push_str(piece);
    }

    fn close_open(&mut self) {
        if self.open.take().is_some() {
            self.push("]}");
        }
    }
}

#[async_trait]
impl EntryVisitor for Writer<'_> {
    async fn entry(
        &mut self,
        entry: &Entry,
        _session: &StoredSession,
        _before: WalkCursor,
    ) -> Result<Flow, ApiError> {
        let Entry::Message(message) = entry else {
            return Ok(Flow::Continue);
        };
        let conversation = message.key.conversation_id;
        if self.open == Some(conversation) {
            self.push(",");
        } else {
            self.close_open();
            if self.any {
                self.push(",");
            }
            let name = self
                .names
                .get(&conversation)
                .map(String::as_str)
                .unwrap_or("");
            let header = json!({"uuid": conversation, "name": name}).to_string();
            // The header without its closing brace, then the messages.
            self.push(&header[..header.len() - 1]);
            self.push(",\"chat_messages\":[");
            self.open = Some(conversation);
            self.any = true;
            self.begun += 1;
        }
        self.push(&message_json(message).to_string());
        if message.sender == Sender::Human {
            self.handles.insert(
                message.key.id,
                self.key
                    .handle_for(self.user_id, conversation, message.key.id),
            );
        }
        Ok(if self.sent_bytes >= PART_BYTES {
            Flow::Stop
        } else {
            Flow::Continue
        })
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn export(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(messages): State<Arc<dyn MessageReader>>,
    State(conversations): State<Arc<dyn ConversationSummaryStore>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(flag_handle_key): State<Arc<FlagHandleKey>>,
    State(budget): State<BudgetSetting>,
    Query(query): Query<PartQuery>,
) -> Result<Json<ExportPart>, ApiError> {
    let start = parse_for(query.cursor.as_deref(), "GET /export", |c| match c {
        Cursor::Export { walk, open, any } => Some((walk, open, any)),
        _ => None,
    })?;
    let names: HashMap<ConversationId, String> = conversations
        .list_for_user(&user_id)
        .await?
        .into_iter()
        .map(|s| (s.conversation_id, s.name.0))
        .collect();
    let record = user_records.get(&user_id).await?;
    let mut writer = Writer {
        user_id: &user_id,
        key: &flag_handle_key,
        names: &names,
        text: String::new(),
        sent_bytes: 0,
        open: start.and_then(|(_, open, _)| open),
        any: start.is_some_and(|(_, _, any)| any),
        handles: HashMap::new(),
        begun: 0,
    };
    if start.is_none() {
        writer.push(&format!(
            "{{\"claude_timeline_format_version\":\"{FORMAT_VERSION}\",\"conversations\":["
        ));
    }
    let end = find_messages(
        WalkStores {
            sessions: sessions.as_ref(),
            messages: messages.as_ref(),
        },
        &user_id,
        &MessageFilter::everything(),
        &WalkOrder::Key,
        start.map(|(walk, _, _)| walk),
        &mut budget.start(),
        &mut writer,
    )
    .await?;
    if end.cursor.is_none() {
        writer.close_open();
        writer.push("]}");
    }
    // For this request's log line (crate::request_log).
    note("conversations", writer.begun);
    note("export_bytes", writer.text.len());
    note("sessions_done", end.sessions_done);
    Ok(Json(ExportPart {
        cursor: end.cursor.map(|walk| {
            Cursor::Export {
                walk,
                open: writer.open,
                any: writer.any,
            }
            .to_text()
        }),
        part: writer.text,
        flag_handles: writer.handles,
        sessions_done: end.sessions_done,
        sessions_total: end.sessions_total,
        data_version: record.data_version,
    }))
}
