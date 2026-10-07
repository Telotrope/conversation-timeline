//! What is kept of one parsed conversation (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §2–§4d): the
//! messages on its kept path as stored rows, a note for each replaced
//! branch, important branches as conversations of their own, and the text
//! of every file whose text the export holds.
//!
//! Kept of each message: its text pieces with their citations, marks for
//! the files it presented where it presented them, your attachments, and for
//! your messages your review from the file (`_claude_timeline_user`).
//! Dropped: tool results, every other tool call, thinking. Automatic flags
//! a file carries (`_claude_timeline_auto`) are never taken: the scan
//! writes those.

use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::branches::{prune_replaced_branches, ReplacedBranch};
use crate::dedup::dedup_chat_messages;
use crate::flag_values::{FlagOverrides, MessageFlags};
use crate::kept_files::{base_name, kind_of, tool_call, FileReplay};
use crate::labels::FileName;
use crate::model::{
    ChatMessage, Conversation, ConversationId, ConversationName, MessageId, PieceType, Sender,
};
use crate::stored_message::{
    BranchNote, Citation, CitedAddress, Entry, EntryKey, FileContents, FileKind, FileRef, Piece,
    Position, StoredMessage,
};

/// The field an annotated download carries your review in.
pub const REVIEW_FIELD: &str = "_claude_timeline_user";

/// A file's text, to be stored under its message and number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptFile {
    pub message_id: MessageId,
    pub number: usize,
    pub text: String,
}

/// One conversation's kept rows and files.
#[derive(Debug, Clone, PartialEq)]
pub struct KeptConversation {
    pub conversation_id: ConversationId,
    pub name: ConversationName,
    /// Messages and notes, in time order.
    pub entries: Vec<Entry>,
    pub files: Vec<KeptFile>,
    /// The messages on the kept path, in order, for a later file to be
    /// compared against.
    pub kept_path: Vec<ChatMessage>,
}

/// An important branch, kept as a conversation of its own (§4d).
#[derive(Debug, Clone, PartialEq)]
pub struct KeptBranch {
    pub conversation: KeptConversation,
    /// The conversation it branched from.
    pub branch_of: ConversationId,
}

/// Everything kept from one conversation of an upload.
#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub main: KeptConversation,
    pub branches: Vec<KeptBranch>,
}

/// Why a conversation couldn't be kept.
#[derive(Debug)]
pub enum KeepError {
    /// A message's review field is not a review this project wrote: not an
    /// object of optional caps/critical/angry booleans.
    Review {
        message_id: MessageId,
        error: serde_json::Error,
    },
}

impl fmt::Display for KeepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeepError::Review { message_id, error } => write!(
                f,
                "message {message_id} has an unreadable {REVIEW_FIELD} review: {error}"
            ),
        }
    }
}

impl std::error::Error for KeepError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            KeepError::Review { error, .. } => Some(error),
        }
    }
}

/// Keeps `conversation`. A fresh export (`already_processed` false) has its
/// retried resends removed after its replaced branches are pruned; an
/// annotated download this tool wrote was cleaned when it was made.
pub fn keep_conversation(
    conversation: &Conversation,
    already_processed: bool,
) -> Result<Kept, KeepError> {
    let pruned = prune_replaced_branches(conversation);
    let kept_path = if already_processed {
        pruned.kept
    } else {
        dedup_chat_messages(&pruned.kept)
    };
    let positions = file_positions(conversation);
    let mut main = keep_messages(
        conversation.uuid,
        conversation.name.clone(),
        kept_path,
        &positions,
    )?;
    let mut branches = Vec::new();
    for branch in &pruned.branches {
        // Pruning never makes an empty branch.
        let first = &branch.messages[0];
        let kept_as = if branch.is_important() {
            let kept = keep_branch(conversation, branch, &positions)?;
            let id = kept.conversation.conversation_id;
            branches.push(kept);
            Some(id)
        } else {
            None
        };
        main.entries.push(note_for(
            conversation.uuid,
            first,
            branch,
            kept_as,
            &positions,
        ));
    }
    main.entries.sort_by_key(Entry::key);
    Ok(Kept { main, branches })
}

/// The note standing where `branch` began.
pub fn note_for(
    conversation_id: ConversationId,
    first: &ChatMessage,
    branch: &ReplacedBranch,
    kept_as: Option<ConversationId>,
    positions: &FilePositions,
) -> Entry {
    let last_at = branch
        .messages
        .iter()
        .map(|m| m.created_at)
        .fold(first.created_at, DateTime::max);
    Entry::Note(BranchNote {
        key: EntryKey {
            conversation_id,
            position: positions.of(first.uuid),
            at: first.created_at,
            id: first.uuid,
        },
        last_at,
        messages: branch.messages.len(),
        words_not_repeated: branch.words_not_repeated,
        replaced_by: branch.replaced_by,
        kept_as,
    })
}

/// An important branch as a conversation of its own: named
/// "{name}: earlier branch from {date, time}" (in UTC, since the server
/// doesn't know the viewer's zone), with an id made from the conversation's
/// and the branch's first message's, so processing it again gives the same.
fn keep_branch(
    conversation: &Conversation,
    branch: &ReplacedBranch,
    positions: &FilePositions,
) -> Result<KeptBranch, KeepError> {
    let first = &branch.messages[0];
    let id = branch_conversation_id(conversation.uuid, first.uuid);
    // Pruning leaves a conversation with any message of unknown time whole,
    // so a branch's messages all have times.
    let when = first.created_at.format("%Y-%m-%d %H:%M UTC");
    let name = ConversationName(format!(
        "{}: earlier branch from {when}",
        conversation.name.0
    ));
    let kept = keep_messages(id, name, branch.messages.clone(), positions)?;
    Ok(KeptBranch {
        conversation: kept,
        branch_of: conversation.uuid,
    })
}

/// The id a branch kept as its own conversation gets: a name-based id
/// (version 5) from the conversation's id and its first message's.
pub fn branch_conversation_id(conversation: ConversationId, first: MessageId) -> ConversationId {
    ConversationId(Uuid::new_v5(&conversation.0, first.0.as_bytes()))
}

/// Each message's place in its conversation's `chat_messages` list, the
/// order the file gives (plan §12.3).
pub struct FilePositions(HashMap<MessageId, Position>);

impl FilePositions {
    /// `id`'s position. Every message kept comes from the list the
    /// positions were read from.
    fn of(&self, id: MessageId) -> Position {
        // Unreachable backstop: kept messages and notes are all taken from
        // the conversation the positions were read from.
        *self
            .0
            .get(&id)
            .expect("a kept message is in its file's list")
    }
}

/// The positions of `conversation`'s messages in its file.
pub fn file_positions(conversation: &Conversation) -> FilePositions {
    FilePositions(
        conversation
            .chat_messages
            .iter()
            .enumerate()
            .map(|(i, m)| (m.uuid, Position(i as i64)))
            .collect(),
    )
}

/// The rows and files of `messages`, all of one conversation, ordered by
/// their `positions` in the file.
pub fn keep_messages(
    conversation_id: ConversationId,
    name: ConversationName,
    messages: Vec<ChatMessage>,
    positions: &FilePositions,
) -> Result<KeptConversation, KeepError> {
    let replay = FileReplay::of(&messages);
    let mut entries = Vec::with_capacity(messages.len());
    let mut files = Vec::new();
    for message in &messages {
        let (stored, message_files) = keep_message(
            conversation_id,
            positions.of(message.uuid),
            message,
            &replay,
        )?;
        entries.push(Entry::Message(stored));
        files.extend(message_files);
    }
    entries.sort_by_key(Entry::key);
    Ok(KeptConversation {
        conversation_id,
        name,
        entries,
        files,
        kept_path: messages,
    })
}

/// Numbers a message's files from 0 as they are met.
struct Numbering {
    message_id: MessageId,
    next: usize,
    files: Vec<KeptFile>,
}

impl Numbering {
    fn file(&mut self, name: &str, kind: FileKind, text: Option<String>, changed: bool) -> FileRef {
        let number = self.next;
        self.next += 1;
        let contents = match text {
            Some(text) => {
                self.files.push(KeptFile {
                    message_id: self.message_id,
                    number,
                    text,
                });
                FileContents::Stored
            }
            None => FileContents::NotInExport,
        };
        FileRef {
            number,
            name: file_name(name, number),
            kind,
            contents,
            may_have_changed_later: changed,
        }
    }
}

/// A file's name cleaned as a label; an empty one is called by its number.
fn file_name(raw: &str, number: usize) -> FileName {
    FileName::parse(raw).unwrap_or_else(|_| {
        // Unreachable backstop: a name made of a number is never empty.
        FileName::parse(&format!("file {}", number + 1)).expect("a numbered name is a label")
    })
}

fn keep_message(
    conversation_id: ConversationId,
    position: Position,
    message: &ChatMessage,
    replay: &FileReplay,
) -> Result<(StoredMessage, Vec<KeptFile>), KeepError> {
    let mut numbering = Numbering {
        message_id: message.uuid,
        next: 0,
        files: Vec::new(),
    };
    let mut pieces = Vec::new();
    for piece in &message.content {
        if piece.piece_type == PieceType::Text {
            pieces.push(Piece::Text {
                text: piece.text.clone(),
                citations: citations_of(piece.extra.get("citations")),
            });
            continue;
        }
        let Some((tool, input)) = tool_call(piece) else {
            continue;
        };
        match tool {
            "present_files" => {
                let paths = input.get("filepaths").and_then(Value::as_array);
                for path in paths.into_iter().flatten().filter_map(Value::as_str) {
                    let name = base_name(path);
                    let file = match replay.get(path) {
                        Some(f) => numbering.file(
                            name,
                            kind_of(name),
                            Some(f.text.clone()),
                            f.may_have_changed_later,
                        ),
                        None => numbering.file(name, kind_of(name), None, false),
                    };
                    pieces.push(Piece::File { file });
                }
            }
            "visualize:show_widget" => {
                let Some(code) = input.get("widget_code").and_then(Value::as_str) else {
                    continue;
                };
                let title = input
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("widget");
                let file = numbering.file(
                    &format!("{title}.html"),
                    FileKind::WebPage,
                    Some(code.to_string()),
                    false,
                );
                pieces.push(Piece::File { file });
            }
            _ => {}
        }
    }
    let mut attachments = Vec::new();
    let listed = |field: &str| {
        message
            .extra
            .get(field)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    for attachment in listed("attachments") {
        let name = attachment
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        let text = attachment
            .get("extracted_content")
            .and_then(Value::as_str)
            .map(str::to_string);
        attachments.push(numbering.file(name, FileKind::Text, text, false));
    }
    for upload in listed("files") {
        let name = upload
            .get("file_name")
            .and_then(Value::as_str)
            .unwrap_or("");
        attachments.push(numbering.file(name, kind_of(name), None, false));
    }
    let flags = match message.sender {
        Sender::Human => Some(MessageFlags {
            auto: None,
            user: review_of(message)?,
        }),
        _ => None,
    };
    let stored = StoredMessage {
        key: EntryKey {
            conversation_id,
            position,
            at: message.created_at,
            id: message.uuid,
        },
        parent: match message.parent() {
            crate::model::ParentLink::Message(id) => Some(id),
            _ => None,
        },
        sender: message.sender.clone(),
        pieces,
        attachments,
        flags,
    };
    Ok((stored, numbering.files))
}

/// Your review from the file: an object of optional booleans, or nothing.
fn review_of(message: &ChatMessage) -> Result<FlagOverrides, KeepError> {
    match message.extra.get(REVIEW_FIELD) {
        None => Ok(FlagOverrides::default()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| KeepError::Review {
            message_id: message.uuid,
            error,
        }),
    }
}

/// A text piece's citations: each a span of the text and the address it
/// came from. A citation without an address points nowhere and is left out.
fn citations_of(value: Option<&Value>) -> Vec<Citation> {
    let Some(list) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| {
            let index = |name: &str| c.get(name).and_then(Value::as_u64).map(|n| n as usize);
            let address = c.get("details")?.get("url")?.as_str()?;
            Some(Citation {
                start: index("start_index")?,
                end: index("end_index")?,
                address: CitedAddress::parse(address),
            })
        })
        .collect()
}

/// The latest known time among `messages`.
pub fn latest_time(messages: &[ChatMessage]) -> Option<DateTime<Utc>> {
    messages.iter().filter_map(|m| m.time().known()).max()
}
