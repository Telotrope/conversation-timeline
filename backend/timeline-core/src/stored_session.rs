//! A session as stored (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3, §6): a run
//! of one conversation's entries with no pause of 15 minutes or more,
//! carrying the fourteen counts that let the Calendar, the Conversations
//! tab, the flag filter and three analyses work without reading messages.
//!
//! Cutting uses the same rule as [`crate::sessions::build_blocks`]
//! ([`GAP_THRESHOLD_SEC`]), over entries rather than a parsed export, and a
//! branch note's whole span counts as activity, so pruning a branch never
//! splits a session the user was working through.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::flag_values::{FlagKind, MessageFlags};
use crate::flag_view::FlagView;
use crate::model::ConversationId;
use crate::sessions::GAP_THRESHOLD_SEC;
use crate::stored_message::Entry;

/// For one view: your messages showing each flag, and any of the three.
/// "Any" is stored, not added up, since one message can carry two flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ViewCounts {
    pub caps: usize,
    pub critical: usize,
    pub angry: usize,
    pub any: usize,
}

impl ViewCounts {
    pub fn of(&self, kind: FlagKind) -> usize {
        match kind {
            FlagKind::Caps => self.caps,
            FlagKind::Critical => self.critical,
            FlagKind::Angry => self.angry,
        }
    }

    fn add(&mut self, view: FlagView, flags: &MessageFlags) {
        if view.shows(flags, FlagKind::Caps) {
            self.caps += 1;
        }
        if view.shows(flags, FlagKind::Critical) {
            self.critical += 1;
        }
        if view.shows(flags, FlagKind::Angry) {
            self.angry += 1;
        }
        if view.is_flagged(flags) {
            self.any += 1;
        }
    }
}

/// The fourteen numbers each session stores (§6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionCounts {
    /// Your messages in the session.
    pub messages: usize,
    /// Those with a value of yours for any flag: a rate's denominator when
    /// only your flags are shown, and the "overridden" filter's set.
    pub reviewed: usize,
    pub automatic: ViewCounts,
    pub yours: ViewCounts,
    pub both: ViewCounts,
}

impl SessionCounts {
    /// Counts the flags of `entries`; Claude's messages and notes add
    /// nothing.
    pub fn of<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Self {
        let mut counts = SessionCounts::default();
        for flags in entries.into_iter().filter_map(Entry::your_flags) {
            counts.messages += 1;
            if flags.user.is_review() {
                counts.reviewed += 1;
            }
            counts.automatic.add(FlagView::Automatic, flags);
            counts.yours.add(FlagView::Yours, flags);
            counts.both.add(FlagView::Both, flags);
        }
        counts
    }

    /// The counts under `view`; all zero with neither switch on.
    pub fn view(&self, view: FlagView) -> ViewCounts {
        match view {
            FlagView::Automatic => self.automatic,
            FlagView::Yours => self.yours,
            FlagView::Both => self.both,
            FlagView::Neither => ViewCounts::default(),
        }
    }

    /// The messages a rate under `view` is taken over (`countsTowardRates`).
    pub fn counted(&self, view: FlagView) -> usize {
        match view {
            FlagView::Automatic | FlagView::Both => self.messages,
            FlagView::Yours => self.reviewed,
            FlagView::Neither => 0,
        }
    }
}

/// One stored session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSession {
    pub conversation_id: ConversationId,
    /// From 0, in time order within the conversation.
    pub number: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    /// Every message in the session, yours and Claude's.
    pub message_count: usize,
    pub counts: SessionCounts,
}

impl StoredSession {
    /// Whether `at` falls within the session, ends included.
    pub fn contains(&self, at: DateTime<Utc>) -> bool {
        self.start <= at && at <= self.end
    }
}

/// Cuts one conversation's entries into sessions, oldest first: a pause of
/// [`GAP_THRESHOLD_SEC`] or more between one entry's activity ending and the
/// next one's starting begins a new session.
pub fn cut_sessions(conversation_id: ConversationId, entries: &[Entry]) -> Vec<StoredSession> {
    let mut sorted: Vec<&Entry> = entries.iter().collect();
    sorted.sort_by_key(|e| e.activity().0);

    let mut runs: Vec<Vec<&Entry>> = Vec::new();
    let mut run_end: Option<DateTime<Utc>> = None;
    for entry in sorted {
        let (start, end) = entry.activity();
        match (run_end, runs.last_mut()) {
            (Some(previous_end), Some(run))
                if (start - previous_end).num_seconds() < GAP_THRESHOLD_SEC =>
            {
                run.push(entry);
                run_end = Some(previous_end.max(end));
            }
            _ => {
                runs.push(vec![entry]);
                run_end = Some(end);
            }
        }
    }

    runs.into_iter()
        .enumerate()
        .map(|(number, run)| {
            // Unreachable backstop: a run is only made with an entry in it.
            let start = run
                .iter()
                .map(|e| e.activity().0)
                .min()
                .expect("a run has an entry");
            let end = run
                .iter()
                .map(|e| e.activity().1)
                .max()
                .expect("a run has an entry");
            StoredSession {
                conversation_id,
                number,
                start,
                end,
                message_count: run.iter().filter(|e| e.as_message().is_some()).count(),
                counts: SessionCounts::of(run.iter().copied()),
            }
        })
        .collect()
}
