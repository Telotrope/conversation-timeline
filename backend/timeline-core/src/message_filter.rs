//! Review's filters as one value, shared by every route that reads messages
//! (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b).
//! Built once from a request, it answers two questions: can a session hold
//! a match (so sessions that can't are never read), and does a message
//! match. The rules are the page's `getFilteredHumanMessages`
//! (`frontend/ui/views/review.js`), moved here.

use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::flag_values::FlagKind;
use crate::flag_view::FlagView;
use crate::model::{ConversationId, Sender};
use crate::stored_message::Entry;
use crate::stored_session::StoredSession;

/// The most characters a search keeps.
pub const SEARCH_CAP: usize = 200;

/// Letters a message's text must contain, ignoring capitals: trimmed,
/// lowercased, never empty, at most [`SEARCH_CAP`] characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchText(String);

impl SearchText {
    /// `None` for text that is empty once trimmed: no search.
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self(
            trimmed
                .chars()
                .take(SEARCH_CAP)
                .collect::<String>()
                .to_lowercase(),
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A time span, ends included, that never ends before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSpan {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndBeforeStart;

impl fmt::Display for EndBeforeStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a time span can't end before it starts")
    }
}

impl std::error::Error for EndBeforeStart {}

impl TimeSpan {
    pub fn new(start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Self, EndBeforeStart> {
        if end < start {
            return Err(EndBeforeStart);
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> DateTime<Utc> {
        self.start
    }

    pub fn end(&self) -> DateTime<Utc> {
        self.end
    }

    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.start <= at && at <= self.end
    }

    pub fn overlaps(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> bool {
        start <= self.end && self.start <= end
    }
}

/// Review's flag menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagFilter {
    #[default]
    All,
    /// Any of the three flags.
    Flagged,
    Only(FlagKind),
    /// A value of yours for any flag (the page's `isOverridden`; the same
    /// set as reviewed).
    Overridden,
}

/// Which messages a request wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageFilter {
    pub conversation: Option<ConversationId>,
    pub span: Option<TimeSpan>,
    pub flag: FlagFilter,
    pub search: Option<SearchText>,
    pub view: FlagView,
    /// Whether Claude's messages and notes match too, by conversation and
    /// span alone. Review lists your messages only; the scan, a session
    /// recount and the annotated download read every entry.
    pub every_entry: bool,
}

impl MessageFilter {
    /// Every entry of every conversation.
    pub fn everything() -> Self {
        Self {
            conversation: None,
            span: None,
            flag: FlagFilter::All,
            search: None,
            view: FlagView::Both,
            every_entry: true,
        }
    }

    /// Every entry of one conversation.
    pub fn conversation(conversation_id: ConversationId) -> Self {
        Self {
            conversation: Some(conversation_id),
            ..Self::everything()
        }
    }

    /// Every entry of one session.
    pub fn session(session: &StoredSession) -> Self {
        Self {
            conversation: Some(session.conversation_id),
            // Unreachable backstop: a stored session never ends before it
            // starts, since it is cut from times in order.
            span: Some(
                TimeSpan::new(session.start, session.end).expect("sessions end after starting"),
            ),
            ..Self::everything()
        }
    }

    /// Whether `session` can hold a match. The search text can't rule a
    /// session out (plan C17); everything else can.
    pub fn admits_session(&self, session: &StoredSession) -> bool {
        if self
            .conversation
            .is_some_and(|c| c != session.conversation_id)
        {
            return false;
        }
        if self
            .span
            .is_some_and(|s| !s.overlaps(session.start, session.end))
        {
            return false;
        }
        let counts = session.counts.view(self.view);
        match self.flag {
            FlagFilter::All => true,
            FlagFilter::Flagged => counts.any > 0,
            FlagFilter::Only(kind) => counts.of(kind) > 0,
            FlagFilter::Overridden => session.counts.reviewed > 0,
        }
    }

    /// Whether `entry` matches every filter.
    pub fn admits(&self, entry: &Entry) -> bool {
        let key = entry.key();
        if self.conversation.is_some_and(|c| c != key.conversation_id) {
            return false;
        }
        if self.span.is_some_and(|s| !s.contains(key.at)) {
            return false;
        }
        if self.every_entry {
            return true;
        }
        let Some(message) = entry.as_message() else {
            return false;
        };
        if message.sender != Sender::Human {
            return false;
        }
        let flags = message.flags.unwrap_or_default();
        let flag_ok = match self.flag {
            FlagFilter::All => true,
            FlagFilter::Flagged => self.view.is_flagged(&flags),
            FlagFilter::Only(kind) => self.view.shows(&flags, kind),
            FlagFilter::Overridden => flags.user.is_review(),
        };
        flag_ok
            && self
                .search
                .as_ref()
                .is_none_or(|s| message.text().to_lowercase().contains(s.as_str()))
    }
}
