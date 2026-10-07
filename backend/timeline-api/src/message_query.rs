//! Finding messages: the one way every route reads message rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b), so a fix to
//! the reading is made once.
//!
//! [`find_messages`] lists the user's sessions (one query), keeps those the
//! filter admits, reads each one's rows with one range query (the time is
//! in the key), and hands every row the filter admits to a visitor. It works
//! within a [`WorkBudget`] (§8c): before each row it asks the budget whether
//! it may go on, and when it may not, it stops and returns a [`WalkCursor`]
//! saying where to carry on. A visitor can also stop it, after the row it
//! was given (Review stops once it has its page of rows).
//!
//! **Order.** Sessions are walked one at a time, each session's rows in
//! the file's order (their keys); messages of two conversations are never
//! interleaved (plan §12.7):
//! - [`WalkOrder::Key`]: sessions in key order (conversation, then number).
//!   Any order would do for the scan, the download and the two server
//!   analyses.
//! - [`WalkOrder::Time`]: sessions by start, then conversation and number:
//!   Review, in every view.
//!
//! A cursor names its session and the last row done in it; a cursor naming
//! a session no longer among the results is refused, since the data
//! changed under it.

use async_trait::async_trait;
use timeline_core::message_filter::MessageFilter;
use timeline_core::ports::ids::UserId;
use timeline_core::ports::messages::{EntryRange, MessageReader};
use timeline_core::ports::sessions::SessionStore;
use timeline_core::stored_message::{Entry, EntryKey};
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::WorkBudget;

use crate::error::ApiError;

/// How sessions are ordered; see the module doc.
pub enum WalkOrder {
    Key,
    Time,
}

/// What a visitor wants after a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Receives the rows a walk finds.
#[async_trait]
pub trait EntryVisitor: Send {
    /// One admitted row, of `session`. `before` is a cursor that would carry
    /// on with this very row: where a page of results starting here begins.
    async fn entry(
        &mut self,
        entry: &Entry,
        session: &StoredSession,
        before: WalkCursor,
    ) -> Result<Flow, ApiError>;

    /// Every row of `group` has been visited.
    async fn group_done(&mut self, _group: &[StoredSession]) -> Result<(), ApiError> {
        Ok(())
    }
}

/// How a walk ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkEnd {
    /// Where to carry on; `None` once every session has been read.
    pub cursor: Option<WalkCursor>,
    /// Sessions in groups finished, of those the filter admitted.
    pub sessions_done: usize,
    pub sessions_total: usize,
}

/// The stores a walk reads.
#[derive(Clone, Copy)]
pub struct WalkStores<'a> {
    pub sessions: &'a dyn SessionStore,
    pub messages: &'a dyn MessageReader,
}

/// Orders the admitted sessions, each a group of its own.
fn groups(mut sessions: Vec<StoredSession>, order: &WalkOrder) -> Vec<Vec<StoredSession>> {
    match order {
        WalkOrder::Key => sessions.sort_by_key(StoredSession::key),
        WalkOrder::Time => sessions.sort_by_key(|s| (s.start, s.key())),
    }
    sessions.into_iter().map(|s| vec![s]).collect()
}

/// Walks the rows `filter` admits; see the module doc. `start` is where an
/// earlier part stopped.
pub async fn find_messages(
    stores: WalkStores<'_>,
    user_id: &UserId,
    filter: &MessageFilter,
    order: &WalkOrder,
    start: Option<WalkCursor>,
    budget: &mut WorkBudget,
    visitor: &mut dyn EntryVisitor,
) -> Result<WalkEnd, ApiError> {
    let admitted: Vec<StoredSession> = stores
        .sessions
        .list_sessions(user_id)
        .await?
        .into_iter()
        .filter(|s| filter.admits_session(s))
        .collect();
    let sessions_total = admitted.len();
    let groups = groups(admitted, order);
    let first_group = match start {
        None => 0,
        Some(cursor) => groups
            .iter()
            .position(|g| g[0].key() == cursor.group)
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "the cursor names a session that no longer starts a part of these results; \
                     the data changed, so start again"
                        .to_string(),
                )
            })?,
    };
    let mut sessions_done: usize = groups[..first_group].iter().map(Vec::len).sum();
    let mut resume_after = start.and_then(|c| c.after);
    for (index, group) in groups.iter().enumerate().skip(first_group) {
        let mut rows: Vec<(Entry, &StoredSession)> = Vec::new();
        for session in group {
            let range = EntryRange::session(session);
            for entry in stores.messages.read_entries(user_id, range).await? {
                rows.push((entry, session));
            }
        }
        rows.sort_by_key(|(entry, _)| entry.key());
        let after = resume_after.take();
        let mut last_done = after;
        let here = |after: Option<EntryKey>| WalkCursor {
            group: group[0].key(),
            after,
        };
        let mut stopped = false;
        for (entry, session) in rows
            .iter()
            .filter(|(e, _)| after.is_none_or(|a| e.key() > a))
        {
            if stopped {
                // A visitor stopped on the row before this one.
                return Ok(WalkEnd {
                    cursor: Some(here(last_done)),
                    sessions_done,
                    sessions_total,
                });
            }
            if !budget.take_step() {
                return Ok(WalkEnd {
                    cursor: Some(here(last_done)),
                    sessions_done,
                    sessions_total,
                });
            }
            let before = here(last_done);
            last_done = Some(entry.key());
            if !filter.admits(entry, session) {
                continue;
            }
            if visitor.entry(entry, session, before).await? == Flow::Stop {
                stopped = true;
            }
        }
        visitor.group_done(group).await?;
        sessions_done += group.len();
        if stopped {
            return Ok(WalkEnd {
                cursor: groups.get(index + 1).map(|next| WalkCursor {
                    group: next[0].key(),
                    after: None,
                }),
                sessions_done,
                sessions_total,
            });
        }
    }
    Ok(WalkEnd {
        cursor: None,
        sessions_done,
        sessions_total,
    })
}

/// Every row of `session`, through [`find_messages`], with no time limit:
/// a flag save's or the scan's recount (§6), never split.
pub async fn session_entries(
    stores: WalkStores<'_>,
    user_id: &UserId,
    session: &StoredSession,
) -> Result<Vec<Entry>, ApiError> {
    struct Collect(Vec<Entry>);
    #[async_trait]
    impl EntryVisitor for Collect {
        async fn entry(
            &mut self,
            entry: &Entry,
            _session: &StoredSession,
            _before: WalkCursor,
        ) -> Result<Flow, ApiError> {
            self.0.push(entry.clone());
            Ok(Flow::Continue)
        }
    }
    let mut collect = Collect(Vec::new());
    find_messages(
        stores,
        user_id,
        &MessageFilter::session(session),
        &WalkOrder::Key,
        None,
        &mut WorkBudget::unlimited(),
        &mut collect,
    )
    .await?;
    Ok(collect.0)
}
