//! A message's time, which may be unknown (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4e).
//!
//! An export can hold messages with no `created_at`. Such a message is kept,
//! with its time unknown. Where a time has to be written down (a parsed
//! message's `created_at`, a stored row's key), unknown is written as the
//! zero date, [`UNKNOWN_TIME`]. Code never compares a time against that
//! sentinel itself: it asks for a [`MessageTime`], so no reader can mistake
//! the sentinel for a real time. A real message sent at exactly that instant
//! would be read as unknown; no Claude conversation is that old.

use chrono::{DateTime, Utc};

/// The zero date, `1970-01-01T00:00:00Z`, written where a time is unknown.
pub const UNKNOWN_TIME: DateTime<Utc> = DateTime::UNIX_EPOCH;

/// When a message was sent, if the export said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MessageTime {
    Unknown,
    Known(DateTime<Utc>),
}

impl MessageTime {
    /// Reads a written-down time back: the sentinel is unknown.
    pub fn from_written(at: DateTime<Utc>) -> Self {
        if at == UNKNOWN_TIME {
            MessageTime::Unknown
        } else {
            MessageTime::Known(at)
        }
    }

    /// The time as written down: the sentinel when unknown.
    pub fn written(self) -> DateTime<Utc> {
        match self {
            MessageTime::Known(at) => at,
            MessageTime::Unknown => UNKNOWN_TIME,
        }
    }

    pub fn known(self) -> Option<DateTime<Utc>> {
        match self {
            MessageTime::Known(at) => Some(at),
            MessageTime::Unknown => None,
        }
    }
}
