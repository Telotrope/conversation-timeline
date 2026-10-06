//! Where a walk over the user's messages stopped (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8c): the group
//! of sessions it was in, named by the group's first session, and the last
//! entry done within it. A walk that answers in parts hands this back, and
//! the next part carries on from it.

use serde::{Deserialize, Serialize};

use crate::stored_message::EntryKey;
use crate::stored_session::SessionKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkCursor {
    /// The first session of the group the walk was in.
    pub group: SessionKey,
    /// The last entry done in that group, in the group's order; `None` when
    /// none was done yet.
    pub after: Option<EntryKey>,
}
