//! Builders for stored entries, so each test states only what it cares
//! about.

#![allow(dead_code)]

use chrono::{DateTime, Utc};
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::model::{ConversationId, MessageId, Sender};
use timeline_core::stored_message::{BranchNote, Entry, EntryKey, Piece, Position, StoredMessage};

pub fn conv(n: u128) -> ConversationId {
    ConversationId(uuid::Uuid::from_u128(n))
}

/// Minutes after a fixed start: 2023-11-14 22:13:20 UTC.
pub fn minute(m: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_700_000_000 + m * 60, 0).unwrap()
}

/// A key whose position follows its time, so entries built with it sit in
/// the file in time order; [`key_at`] places one anywhere (plan §12.3).
pub fn key(conversation: u128, at: DateTime<Utc>, id: u128) -> EntryKey {
    key_at(conversation, at.timestamp(), at, id)
}

pub fn key_at(conversation: u128, position: i64, at: DateTime<Utc>, id: u128) -> EntryKey {
    EntryKey {
        conversation_id: conv(conversation),
        position: Position(position),
        at,
        id: MessageId(uuid::Uuid::from_u128(id)),
    }
}

pub fn message(key: EntryKey, sender: Sender, text: &str, flags: Option<MessageFlags>) -> Entry {
    Entry::Message(StoredMessage {
        key,
        parent: None,
        sender,
        pieces: vec![Piece::Text {
            text: text.to_string(),
            citations: Vec::new(),
        }],
        attachments: Vec::new(),
        flags,
    })
}

pub fn yours(key: EntryKey, flags: MessageFlags) -> Entry {
    message(key, Sender::Human, "hello", Some(flags))
}

pub fn yours_saying(key: EntryKey, text: &str) -> Entry {
    message(key, Sender::Human, text, Some(MessageFlags::default()))
}

pub fn claudes(key: EntryKey) -> Entry {
    message(key, Sender::Assistant, "a reply", None)
}

pub fn note(key: EntryKey, last_at: DateTime<Utc>) -> Entry {
    Entry::Note(BranchNote {
        key,
        last_at,
        messages: 1,
        words_not_repeated: 0,
        replaced_by: None,
        kept_as: None,
    })
}

pub fn auto(caps: bool, critical: bool, angry: bool) -> MessageFlags {
    MessageFlags {
        auto: Some(FlagSet {
            caps,
            critical,
            angry,
        }),
        user: FlagOverrides::default(),
    }
}

pub fn reviewed(caps: Option<bool>, critical: Option<bool>, angry: Option<bool>) -> MessageFlags {
    MessageFlags {
        auto: None,
        user: FlagOverrides {
            caps,
            critical,
            angry,
        },
    }
}
