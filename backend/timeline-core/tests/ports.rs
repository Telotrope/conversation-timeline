//! Small pieces of the storage ports that carry rules of their own (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4e, §8b).

#[path = "support/entries.rs"]
mod entries;

use entries::*;
use timeline_core::conversation_metadata::ConversationSpan;
use timeline_core::ports::messages::EntryRange;
use timeline_core::stored_session::{cut_sessions, sessions_for};
use timeline_core::UNKNOWN_TIME;

/// A session cut by pauses is read by its range of positions (plan §12.3);
/// a session placed by its conversation's start and end reads the whole
/// conversation (§4e).
#[test]
fn a_sessions_rows_are_its_positions_or_its_whole_conversation() {
    let timed = cut_sessions(
        conv(1),
        &[yours(key(1, minute(3), 1), auto(false, false, false))],
    );
    assert_eq!(
        EntryRange::session(&timed[0]),
        EntryRange {
            conversation_id: conv(1),
            positions: Some((key(1, minute(3), 1).position, key(1, minute(3), 1).position)),
            after: None,
        }
    );
    let span = ConversationSpan::new(minute(0).fixed_offset(), minute(9).fixed_offset()).unwrap();
    let placed = sessions_for(
        conv(2),
        &[yours(key(2, UNKNOWN_TIME, 1), auto(false, false, false))],
        &span,
    );
    assert_eq!(
        EntryRange::session(&placed[0]),
        EntryRange {
            conversation_id: conv(2),
            positions: None,
            after: None,
        }
    );
}
