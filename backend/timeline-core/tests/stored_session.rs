//! Sessions as stored (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4e, §6): cut by
//! pauses of 15 minutes or more, or one session placed by the conversation's
//! start and end when any message has no time; and the fourteen counts.

#[path = "support/entries.rs"]
mod entries;

use chrono::Duration;
use entries::*;
use timeline_core::conversation_metadata::ConversationSpan;
use timeline_core::flag_values::FlagKind;
use timeline_core::flag_view::FlagView;
use timeline_core::stored_session::{
    cut_sessions, sessions_for, Placement, SessionCounts, SessionKey, ViewCounts,
};
use timeline_core::UNKNOWN_TIME;

fn span(from: i64, to: i64) -> ConversationSpan {
    ConversationSpan::new(minute(from).fixed_offset(), minute(to).fixed_offset()).unwrap()
}

/// Replaces `blocks.test.js` L13 ("a gap of 15 minutes or more starts a new
/// session"): a pause of exactly 15 minutes starts one; 14:59 does not.
#[test]
fn a_pause_of_15_minutes_starts_a_session_and_14_59_does_not() {
    let start = minute(0);
    let entries = vec![
        yours(key(1, start, 1), auto(false, false, false)),
        claudes(key(1, start + Duration::seconds(14 * 60 + 59), 2)),
        yours(
            key(1, start + Duration::seconds(29 * 60 + 59), 3),
            auto(false, false, false),
        ),
    ];
    let sessions = cut_sessions(conv(1), &entries);
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].start, start);
    assert_eq!(sessions[0].end, start + Duration::seconds(899));
    assert_eq!(sessions[0].message_count, 2);
    assert_eq!(sessions[0].number, 0);
    assert_eq!(sessions[1].number, 1);
    assert_eq!(sessions[1].start, start + Duration::seconds(1799));
    assert!(sessions.iter().all(|s| s.placement == Placement::Gaps));
}

/// Replaces `blocks.test.js` L27 ("a session continues across midnight, and
/// never spans two conversations"): one session across midnight; two
/// conversations at the same times give separate sessions.
#[test]
fn a_session_continues_across_midnight_and_conversations_are_cut_apart() {
    let before_midnight = chrono::DateTime::parse_from_rfc3339("2026-03-01T23:55:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let first = vec![
        yours(key(1, before_midnight, 1), auto(false, false, false)),
        claudes(key(1, before_midnight + Duration::minutes(10), 2)),
    ];
    let second = vec![yours(
        key(2, before_midnight + Duration::minutes(1), 3),
        auto(false, false, false),
    )];
    let one = cut_sessions(conv(1), &first);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].end - one[0].start, Duration::minutes(10));
    let other = cut_sessions(conv(2), &second);
    assert_eq!(other.len(), 1);
    assert_eq!(other[0].conversation_id, conv(2));
    assert_eq!(one[0].conversation_id, conv(1));
}

/// A replaced branch's note spans its first to last message, and counts as
/// activity (§4d), so it can bridge a pause its messages would have made.
#[test]
fn a_notes_span_counts_as_activity_and_keeps_a_session_whole() {
    let entries = vec![
        yours(key(1, minute(0), 1), auto(false, false, false)),
        note(key(1, minute(10), 2), minute(24)),
        yours(key(1, minute(38), 3), auto(false, false, false)),
    ];
    let sessions = cut_sessions(conv(1), &entries);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].start, minute(0));
    assert_eq!(sessions[0].end, minute(38));
    // A note is not a message.
    assert_eq!(sessions[0].message_count, 2);
    assert_eq!(sessions[0].counts.messages, 2);
}

#[test]
fn a_note_reaching_past_the_next_entry_keeps_the_later_end() {
    let entries = vec![
        note(key(1, minute(0), 2), minute(40)),
        yours(key(1, minute(10), 1), auto(false, false, false)),
        yours(key(1, minute(50), 3), auto(false, false, false)),
    ];
    let sessions = cut_sessions(conv(1), &entries);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].end, minute(50));
}

/// The fourteen counts (§6): your messages, your reviewed messages, and for
/// each view that shows anything each flag and any of the three. Claude's
/// messages and notes count nowhere.
#[test]
fn the_fourteen_counts_follow_each_view() {
    let entries = vec![
        // Automatic caps and critical; you said critical is wrong.
        yours(
            key(1, minute(0), 1),
            timeline_core::flag_values::MessageFlags {
                user: timeline_core::flag_values::FlagOverrides {
                    caps: None,
                    critical: Some(false),
                    angry: None,
                },
                ..auto(true, true, false)
            },
        ),
        // You flagged angry; never scanned.
        yours(key(1, minute(1), 2), reviewed(None, None, Some(true))),
        // Automatic angry, unreviewed.
        yours(key(1, minute(2), 3), auto(false, false, true)),
        // Nothing.
        yours(key(1, minute(3), 4), auto(false, false, false)),
        claudes(key(1, minute(4), 5)),
        note(key(1, minute(5), 6), minute(5)),
    ];
    let counts = SessionCounts::of(&entries);
    assert_eq!(counts.messages, 4);
    assert_eq!(counts.reviewed, 2);
    assert_eq!(
        counts.automatic,
        ViewCounts {
            caps: 1,
            critical: 1,
            angry: 1,
            any: 2
        }
    );
    assert_eq!(
        counts.yours,
        ViewCounts {
            caps: 0,
            critical: 0,
            angry: 1,
            any: 1
        }
    );
    assert_eq!(
        counts.both,
        ViewCounts {
            caps: 1,
            critical: 0,
            angry: 2,
            any: 3
        }
    );
    assert_eq!(counts.view(FlagView::Neither), ViewCounts::default());
    assert_eq!(counts.view(FlagView::Both).of(FlagKind::Angry), 2);
    assert_eq!(counts.view(FlagView::Automatic).of(FlagKind::Caps), 1);
    assert_eq!(counts.view(FlagView::Yours).of(FlagKind::Critical), 0);
    assert_eq!(counts.counted(FlagView::Automatic), 4);
    assert_eq!(counts.counted(FlagView::Both), 4);
    assert_eq!(counts.counted(FlagView::Yours), 2);
    assert_eq!(counts.counted(FlagView::Neither), 0);
}

/// Replaces `blocks-span.test.js` L20 ("a conversation with no message times
/// is one session from its start to its end") and `flags.test.js` L49
/// ("attachFlags files each message under its session, or the nearest
/// one"): a conversation with a message of unknown time is one session
/// from the record's start to its end, and every one of your messages is
/// counted in it, timed ones outside that span included.
#[test]
fn a_conversation_with_a_message_of_unknown_time_is_one_session_placed_by_its_span() {
    let entries = vec![
        yours(key(1, UNKNOWN_TIME, 1), auto(true, false, false)),
        // Timed, but far outside the record's span.
        yours(key(1, minute(5000), 2), auto(false, false, true)),
        claudes(key(1, UNKNOWN_TIME, 3)),
    ];
    let sessions = sessions_for(conv(1), &entries, &span(100, 160));
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert_eq!(session.placement, Placement::Span);
    assert_eq!((session.start, session.end), (minute(100), minute(160)));
    assert_eq!(session.message_count, 3);
    assert_eq!(session.counts.messages, 2);
    assert_eq!(session.counts.both.any, 2);
    assert_eq!(session.duration_sec(), 3600);
}

/// Replaces `blocks-span.test.js` L29 ("editing its start and end moves
/// it"): the session follows the span it is given.
#[test]
fn editing_the_span_moves_the_placed_session() {
    let entries = vec![yours(key(1, UNKNOWN_TIME, 1), auto(false, false, false))];
    let before = sessions_for(conv(1), &entries, &span(0, 60));
    let after = sessions_for(conv(1), &entries, &span(500, 530));
    assert_eq!((before[0].start, before[0].end), (minute(0), minute(60)));
    assert_eq!((after[0].start, after[0].end), (minute(500), minute(530)));
}

/// Replaces `blocks-span.test.js` L35 ("some timed messages and some not:
/// placed by start and end, not by the timed ones").
#[test]
fn some_timed_messages_and_some_not_are_placed_by_the_span_not_the_timed_ones() {
    let entries = vec![
        yours(key(1, minute(0), 1), auto(false, false, false)),
        yours(key(1, minute(100), 2), auto(false, false, false)),
        yours(key(1, UNKNOWN_TIME, 3), auto(false, false, false)),
    ];
    let sessions = sessions_for(conv(1), &entries, &span(40, 50));
    assert_eq!(sessions.len(), 1);
    assert_eq!(
        (sessions[0].start, sessions[0].end),
        (minute(40), minute(50))
    );
}

/// Replaces `blocks-span.test.js` L44 ("timed conversations, empty ones and
/// ones without a record are placed as before"): timed conversations are
/// cut by pauses, and an empty one has no session. ("Without a record" has
/// no match: on the server every conversation has a record.)
#[test]
fn timed_conversations_are_cut_by_pauses_and_empty_ones_have_no_session() {
    let timed = vec![
        yours(key(1, minute(0), 1), auto(false, false, false)),
        yours(key(1, minute(60), 2), auto(false, false, false)),
    ];
    let sessions = sessions_for(conv(1), &timed, &span(0, 600));
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|s| s.placement == Placement::Gaps));
    assert_eq!(sessions_for(conv(1), &[], &span(0, 60)), vec![]);
    // Notes alone are not messages.
    let notes = vec![note(key(1, minute(0), 1), minute(1))];
    assert_eq!(sessions_for(conv(1), &notes, &span(0, 60)), vec![]);
}

#[test]
fn a_session_knows_its_key_and_what_it_contains() {
    let entries = vec![
        yours(key(7, minute(10), 1), auto(false, false, false)),
        yours(key(7, minute(20), 2), auto(false, false, false)),
    ];
    let session = &cut_sessions(conv(7), &entries)[0];
    assert_eq!(
        session.key(),
        SessionKey {
            conversation_id: conv(7),
            number: 0
        }
    );
    assert!(session.contains(minute(10)));
    assert!(session.contains(minute(20)));
    assert!(!session.contains(minute(21)));
    assert!(!session.contains(minute(9)));
}

/// Plan §12.3: sessions are cut in the file's order, and a time stepping
/// backwards starts a session too, so no two overlap; each knows its first
/// and last position.
#[test]
fn a_time_stepping_backwards_starts_a_session_so_none_overlap() {
    let sessions = cut_sessions(
        conv(1),
        &[
            yours(key_at(1, 0, minute(0), 1), auto(false, false, false)),
            yours(key_at(1, 1, minute(10), 2), auto(false, false, false)),
            yours(key_at(1, 2, minute(5), 3), auto(false, false, false)),
            yours(key_at(1, 3, minute(7), 4), auto(false, false, false)),
        ],
    );
    let spans: Vec<_> = sessions
        .iter()
        .map(|s| (s.start, s.end, s.first.0, s.last.0))
        .collect();
    assert_eq!(
        spans,
        vec![(minute(0), minute(10), 0, 1), (minute(5), minute(7), 2, 3)]
    );
}
