//! Keeping a session's fourteen counts current (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §6): a flag save
//! recounts its message's session in the same request, and the scan
//! recounts each session it finishes. Both read the session's rows through
//! [`session_entries`], the shared way of reading message rows, and rewrite
//! the session.

use timeline_core::ports::ids::UserId;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::stored_session::{SessionCounts, StoredSession};

use crate::error::ApiError;
use crate::message_query::{session_entries, WalkStores};

/// Counts `session` again from its rows and stores it; returns it as
/// stored.
pub async fn recount_session(
    stores: WalkStores<'_>,
    session_store: &dyn SessionStore,
    user_id: &UserId,
    session: &StoredSession,
) -> Result<StoredSession, ApiError> {
    let entries = session_entries(stores, user_id, session).await?;
    let recounted = StoredSession {
        counts: SessionCounts::of(&entries),
        ..session.clone()
    };
    session_store
        .put_sessions(user_id, std::slice::from_ref(&recounted))
        .await?;
    Ok(recounted)
}
