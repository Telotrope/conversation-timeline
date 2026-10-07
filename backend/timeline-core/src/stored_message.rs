//! What is kept of a conversation's messages once an upload is processed
//! (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §2–§4d):
//! one entry per message on the kept path, holding its text pieces with
//! their citations and marks for the files it presented, and for yours its
//! flags; and one note per replaced branch. Tool calls, tool results and
//! thinking are not kept.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::flag_values::MessageFlags;
use crate::labels::FileName;
use crate::message_time::MessageTime;
use crate::model::{ConversationId, MessageId, Sender};

/// Where an entry stands in its conversation: the order the file gives it
/// (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §12.3).
/// Within one file it is the message's index in the conversation's
/// `chat_messages` list; a later file's additions are numbered below the
/// first stored position or above the last, so it can be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position(pub i64);

impl Position {
    /// Added so every position written is a positive number of the same
    /// width, so text order is number order.
    const OFFSET: i64 = 1_000_000_000_000;

    /// The position as fixed-width text that sorts as the number does.
    pub fn sort_text(self) -> String {
        format!("{:013}", self.0 + Self::OFFSET)
    }

    /// Reads [`Position::sort_text`] back; `None` for anything it didn't
    /// write.
    pub fn from_sort_text(text: &str) -> Option<Self> {
        if text.len() != 13 || !text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        text.parse::<i64>().ok().map(|n| Position(n - Self::OFFSET))
    }
}

/// Where an entry sits: its conversation, its position and its own id.
/// Entries are stored and read in this order, the file's, so a session's
/// entries are one unbroken run between its first and last position (§3,
/// §12.3). `at` is the entry's time, carried with the key but not deciding
/// its order (positions are unique within a conversation); it is the
/// zero-date sentinel when the time is unknown (§4e), read through
/// [`EntryKey::time`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntryKey {
    pub conversation_id: ConversationId,
    pub position: Position,
    pub at: DateTime<Utc>,
    pub id: MessageId,
}

impl EntryKey {
    pub fn time(&self) -> MessageTime {
        MessageTime::from_written(self.at)
    }
}

/// The web address a citation points to. Only `http` and `https`
/// addresses become links; anything else is kept as text, so a stored
/// citation can never make the page open a `javascript:` address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "address", rename_all = "snake_case")]
pub enum CitedAddress {
    Web(String),
    Other(String),
}

impl CitedAddress {
    pub fn parse(raw: &str) -> Self {
        let lower = raw.trim_start().to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            CitedAddress::Web(raw.trim().to_string())
        } else {
            CitedAddress::Other(raw.to_string())
        }
    }
}

/// A span of a text piece and the address it came from. `start` and `end`
/// are positions in the piece's text exactly as the export gives them
/// (§4c); the page places the marker before formatting the text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Citation {
    pub start: usize,
    pub end: usize,
    pub address: CitedAddress,
}

/// How a file is shown (§4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileKind {
    /// Drawn with an `<img>` element, which runs no scripts in it.
    Svg,
    /// A web page or one of Claude's widgets: laid out in a sandboxed frame
    /// with scripts off.
    WebPage,
    Markdown,
    /// Source code, coloured for `language` (a highlight.js language name).
    Code {
        language: String,
    },
    /// Plain text, shown as is: your attachments, and text files.
    Text,
    /// Anything else (Word, PowerPoint, images Claude made by running
    /// scripts): offered for download only.
    Other,
}

/// Whether a file's contents were in the export, and so stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileContents {
    /// Stored under [`FileRef::number`] for its message.
    Stored,
    /// The export holds only the name.
    NotInExport,
}

/// A file a message carries or presented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Numbers the message's stored files from 0; the file store's key uses
    /// it rather than the name, so no name a file was given ever becomes
    /// part of a storage key.
    pub number: usize,
    pub name: FileName,
    pub kind: FileKind,
    pub contents: FileContents,
    /// A command Claude ran later named this file, so the stored text, made
    /// by replaying Claude's edits, may be older than its final version
    /// (plan C9).
    pub may_have_changed_later: bool,
}

/// One part of a message, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Piece {
    Text {
        text: String,
        citations: Vec<Citation>,
    },
    /// A file the message presented here, between two paragraphs (§4).
    File { file: FileRef },
}

/// One kept message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub key: EntryKey,
    /// The message it answers; `None` for a conversation's first message,
    /// or when the export didn't say.
    pub parent: Option<MessageId>,
    pub sender: Sender,
    pub pieces: Vec<Piece>,
    /// Files you attached or uploaded with the message: those whose text
    /// the export holds are stored; the rest are names only.
    pub attachments: Vec<FileRef>,
    /// Flags, for your messages only; `None` for Claude's.
    pub flags: Option<MessageFlags>,
}

impl StoredMessage {
    /// The message's text pieces joined, as the scan and search read it.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for piece in &self.pieces {
            if let Piece::Text { text: t, .. } = piece {
                text.push_str(t);
            }
        }
        text
    }

    /// Every file the message carries, presented or attached.
    pub fn files(&self) -> impl Iterator<Item = &FileRef> {
        self.pieces
            .iter()
            .filter_map(|p| match p {
                Piece::File { file } => Some(file),
                Piece::Text { .. } => None,
            })
            .chain(self.attachments.iter())
    }
}

/// Stands where a replaced branch began (§4d). Not a message: it counts in
/// no message total, flag count or analysis, but its first and last times
/// count as activity when sessions are cut, so removing a branch can't
/// split a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchNote {
    /// Placed at the branch's first message, with that message's id.
    pub key: EntryKey,
    pub last_at: DateTime<Utc>,
    pub messages: usize,
    pub words_not_repeated: usize,
    /// The kept message that took the branch's place.
    pub replaced_by: Option<MessageId>,
    /// The conversation the branch was kept as, when it was important.
    pub kept_as: Option<ConversationId>,
}

/// How many of `entries`' messages, taken in key order (the file's), are
/// timed earlier than the timed message before them (plan §12.3). Notes and
/// messages of unknown time are passed over.
pub fn count_out_of_order(entries: &[Entry]) -> usize {
    let mut sorted: Vec<&StoredMessage> = entries.iter().filter_map(Entry::as_message).collect();
    sorted.sort_by_key(|m| m.key);
    let mut previous: Option<DateTime<Utc>> = None;
    let mut count = 0;
    for m in sorted {
        let Some(at) = m.key.time().known() else {
            continue;
        };
        if previous.is_some_and(|p| at < p) {
            count += 1;
        }
        previous = Some(at);
    }
    count
}

/// One stored row of a conversation's messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "snake_case")]
pub enum Entry {
    Message(StoredMessage),
    Note(BranchNote),
}

impl Entry {
    pub fn key(&self) -> EntryKey {
        match self {
            Entry::Message(m) => m.key,
            Entry::Note(n) => n.key,
        }
    }

    /// The span of time the entry occupies: an instant for a message, the
    /// branch's first to last message for a note.
    pub fn activity(&self) -> (DateTime<Utc>, DateTime<Utc>) {
        match self {
            Entry::Message(m) => (m.key.at, m.key.at),
            Entry::Note(n) => (n.key.at, n.last_at),
        }
    }

    pub fn as_message(&self) -> Option<&StoredMessage> {
        match self {
            Entry::Message(m) => Some(m),
            Entry::Note(_) => None,
        }
    }

    /// Your message's flags, or `None` for Claude's messages and notes.
    pub fn your_flags(&self) -> Option<&MessageFlags> {
        match self {
            Entry::Message(m) if m.sender == Sender::Human => m.flags.as_ref(),
            _ => None,
        }
    }
}
