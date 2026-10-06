//! Review's filters, shared by every route that reads messages (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b), and how
//! they treat messages of unknown time (§4e).

#[path = "support/entries.rs"]
mod entries;

use entries::*;
use timeline_core::conversation_metadata::ConversationSpan;
use timeline_core::flag_values::FlagKind;
use timeline_core::flag_view::FlagView;
use timeline_core::message_filter::{
    FlagFilter, MessageFilter, SearchText, SpanFilter, SpanKind, TimeSpan, SEARCH_CAP,
};
use timeline_core::stored_session::{cut_sessions, sessions_for, StoredSession};
use timeline_core::UNKNOWN_TIME;

fn span(from: i64, to: i64, kind: SpanKind) -> Option<SpanFilter> {
    Some(SpanFilter {
        span: TimeSpan::new(minute(from), minute(to)).unwrap(),
        kind,
    })
}

fn review(filter: MessageFilter) -> MessageFilter {
    MessageFilter {
        every_entry: false,
        ..filter
    }
}

/// One timed session, holding the entries given.
fn session_of(entries: &[timeline_core::stored_message::Entry]) -> StoredSession {
    cut_sessions(entries[0].key().conversation_id, entries).remove(0)
}

/// The one session of a conversation placed by its span (minutes 100–160).
fn placed_session(entries: &[timeline_core::stored_message::Entry]) -> StoredSession {
    let span =
        ConversationSpan::new(minute(100).fixed_offset(), minute(160).fixed_offset()).unwrap();
    sessions_for(entries[0].key().conversation_id, entries, &span).remove(0)
}

#[test]
fn search_text_is_trimmed_lowercased_and_capped() {
    assert_eq!(SearchText::parse("   "), None);
    assert_eq!(SearchText::parse(""), None);
    assert_eq!(
        SearchText::parse("  WRONG Again ").unwrap().as_str(),
        "wrong again"
    );
    let long = "É".repeat(SEARCH_CAP + 50);
    let parsed = SearchText::parse(&long).unwrap();
    assert_eq!(parsed.as_str().chars().count(), SEARCH_CAP);
    assert!(parsed.as_str().chars().all(|c| c == 'é'));
}

#[test]
fn a_time_span_never_ends_before_it_starts() {
    let err = TimeSpan::new(minute(5), minute(4)).unwrap_err();
    assert_eq!(err.to_string(), "a time span can't end before it starts");
    let span = TimeSpan::new(minute(4), minute(5)).unwrap();
    assert_eq!((span.start(), span.end()), (minute(4), minute(5)));
    assert!(span.contains(minute(4)) && span.contains(minute(5)));
    assert!(!span.contains(minute(6)));
    assert!(span.overlaps(minute(5), minute(9)));
    assert!(span.overlaps(minute(0), minute(4)));
    assert!(!span.overlaps(minute(6), minute(9)));
    assert!(!span.overlaps(minute(0), minute(3)));
    // A single instant is a span too.
    assert!(TimeSpan::new(minute(1), minute(1)).is_ok());
}

#[test]
fn a_session_is_ruled_out_by_conversation_span_or_its_counts() {
    let entries = vec![
        yours(key(1, minute(10), 1), auto(true, false, false)),
        yours(key(1, minute(12), 2), reviewed(None, None, Some(false))),
    ];
    let session = session_of(&entries);
    let all = review(MessageFilter::everything());
    assert!(all.admits_session(&session));
    let other = MessageFilter {
        conversation: Some(conv(2)),
        ..all.clone()
    };
    assert!(!other.admits_session(&session));
    let elsewhere = MessageFilter {
        span: span(20, 30, SpanKind::Range),
        ..all.clone()
    };
    assert!(!elsewhere.admits_session(&session));
    let overlapping = MessageFilter {
        span: span(12, 30, SpanKind::Day),
        ..all.clone()
    };
    assert!(overlapping.admits_session(&session));
    let flag = |flag| MessageFilter {
        flag,
        ..all.clone()
    };
    assert!(flag(FlagFilter::Flagged).admits_session(&session));
    assert!(flag(FlagFilter::Only(FlagKind::Caps)).admits_session(&session));
    assert!(!flag(FlagFilter::Only(FlagKind::Angry)).admits_session(&session));
    assert!(flag(FlagFilter::Overridden).admits_session(&session));
    // Under the view chosen: with only your flags shown, nothing is flagged.
    let yours_only = MessageFilter {
        view: FlagView::Yours,
        ..flag(FlagFilter::Flagged)
    };
    assert!(!yours_only.admits_session(&session));
    // Search can't rule a session out (plan C17).
    let search = MessageFilter {
        search: SearchText::parse("nowhere in it"),
        ..all
    };
    assert!(search.admits_session(&session));
}

#[test]
fn a_session_with_no_reviews_is_ruled_out_for_overridden() {
    let session = session_of(&[yours(key(1, minute(0), 1), auto(true, true, true))]);
    let overridden = MessageFilter {
        flag: FlagFilter::Overridden,
        ..review(MessageFilter::everything())
    };
    assert!(!overridden.admits_session(&session));
}

#[test]
fn a_message_must_match_every_filter() {
    let caps = yours_saying(key(1, minute(10), 1), "This is WRONG");
    let session = session_of(&[caps.clone()]);
    let all = review(MessageFilter::everything());
    assert!(all.admits(&caps, &session));
    assert!(!MessageFilter {
        conversation: Some(conv(2)),
        ..all.clone()
    }
    .admits(&caps, &session));
    assert!(!MessageFilter {
        span: span(11, 20, SpanKind::Range),
        ..all.clone()
    }
    .admits(&caps, &session));
    assert!(MessageFilter {
        span: span(10, 10, SpanKind::Day),
        ..all.clone()
    }
    .admits(&caps, &session));
    // Search ignores capitals.
    assert!(MessageFilter {
        search: SearchText::parse("is wrong"),
        ..all.clone()
    }
    .admits(&caps, &session));
    assert!(!MessageFilter {
        search: SearchText::parse("right"),
        ..all.clone()
    }
    .admits(&caps, &session));
}

#[test]
fn the_flag_menu_follows_the_view() {
    let flagged = yours(key(1, minute(0), 1), auto(false, true, false));
    let mine = yours(key(1, minute(1), 2), reviewed(Some(false), None, None));
    let plain = yours(key(1, minute(2), 3), auto(false, false, false));
    let session = session_of(&[flagged.clone(), mine.clone(), plain.clone()]);
    let with = |flag, view| MessageFilter {
        flag,
        view,
        ..review(MessageFilter::everything())
    };
    let f = with(FlagFilter::Flagged, FlagView::Both);
    assert!(f.admits(&flagged, &session));
    assert!(!f.admits(&mine, &session));
    assert!(!f.admits(&plain, &session));
    let critical = with(FlagFilter::Only(FlagKind::Critical), FlagView::Automatic);
    assert!(critical.admits(&flagged, &session));
    assert!(!critical.admits(&plain, &session));
    assert!(!with(FlagFilter::Flagged, FlagView::Yours).admits(&flagged, &session));
    let overridden = with(FlagFilter::Overridden, FlagView::Both);
    assert!(overridden.admits(&mine, &session));
    assert!(!overridden.admits(&flagged, &session));
}

/// Review lists your messages only; Claude's messages are read by the scan,
/// a recount and the download (`every_entry`).
#[test]
fn claudes_messages_match_only_when_every_entry_is_wanted() {
    let reply = claudes(key(1, minute(0), 1));
    let session = session_of(&[reply.clone()]);
    assert!(!review(MessageFilter::everything()).admits(&reply, &session));
    assert!(MessageFilter::everything().admits(&reply, &session));
    assert!(MessageFilter::conversation(conv(1)).admits(&reply, &session));
    assert!(!MessageFilter::conversation(conv(2)).admits(&reply, &session));
    assert!(!MessageFilter::your_messages(FlagView::Both).admits(&reply, &session));
}

/// Notes are listed among your messages only with the flag menu on "All"
/// and no search: they match no flag and no search.
#[test]
fn notes_are_listed_only_with_all_messages_and_no_search() {
    let a_note = note(key(1, minute(0), 1), minute(1));
    let session = session_of(&[
        yours(key(1, minute(2), 2), auto(false, false, false)),
        a_note.clone(),
    ]);
    let listing = MessageFilter {
        notes: true,
        ..review(MessageFilter::everything())
    };
    assert!(listing.admits(&a_note, &session));
    assert!(!MessageFilter {
        flag: FlagFilter::Flagged,
        ..listing.clone()
    }
    .admits(&a_note, &session));
    assert!(!MessageFilter {
        search: SearchText::parse("pruned"),
        ..listing.clone()
    }
    .admits(&a_note, &session));
    assert!(!review(MessageFilter::everything()).admits(&a_note, &session));
    assert!(MessageFilter::everything().admits(&a_note, &session));
}

/// §4e: a session's span (or an analysis point's) finds every message of a
/// session placed by its conversation's start and end through the session;
/// a Calendar day finds only its timed messages, by their own time.
#[test]
fn messages_of_a_placed_session_are_found_through_the_session_not_by_a_day() {
    let untimed = yours(key(1, UNKNOWN_TIME, 1), auto(false, false, false));
    let timed_outside = yours(key(1, minute(5000), 2), auto(false, false, false));
    let placed = placed_session(&[untimed.clone(), timed_outside.clone()]);
    let all = review(MessageFilter::everything());

    let session_span = MessageFilter {
        span: span(150, 170, SpanKind::Range),
        ..all.clone()
    };
    assert!(session_span.admits_session(&placed));
    assert!(session_span.admits(&untimed, &placed));
    assert!(session_span.admits(&timed_outside, &placed));

    let missing_it = MessageFilter {
        span: span(0, 50, SpanKind::Range),
        ..all.clone()
    };
    assert!(!missing_it.admits_session(&placed));
    assert!(!missing_it.admits(&untimed, &placed));

    // A day can't rule the placed session out by its start and end: its
    // timed messages may lie outside them.
    let day_of_timed = MessageFilter {
        span: span(4990, 5010, SpanKind::Day),
        ..all.clone()
    };
    assert!(day_of_timed.admits_session(&placed));
    assert!(day_of_timed.admits(&timed_outside, &placed));
    assert!(!day_of_timed.admits(&untimed, &placed));
    let day_of_session = MessageFilter {
        span: span(100, 160, SpanKind::Day),
        ..all.clone()
    };
    assert!(!day_of_session.admits(&untimed, &placed));
    // The conversation filter finds every one of them.
    let conversation = MessageFilter {
        conversation: Some(conv(1)),
        ..all
    };
    assert!(conversation.admits(&untimed, &placed));
}

#[test]
fn a_sessions_own_filter_admits_its_entries_and_no_neighbours() {
    let entries = vec![
        yours(key(1, minute(0), 1), auto(false, false, false)),
        claudes(key(1, minute(5), 2)),
        yours(key(1, minute(60), 3), auto(false, false, false)),
    ];
    let sessions = cut_sessions(conv(1), &entries);
    let first = MessageFilter::session(&sessions[0]);
    assert!(first.admits_session(&sessions[0]));
    assert!(!first.admits_session(&sessions[1]));
    assert!(first.admits(&entries[1], &sessions[0]));
    assert!(!first.admits(&entries[2], &sessions[1]));
    // A placed session's filter is its conversation.
    let untimed = yours(key(2, UNKNOWN_TIME, 4), auto(false, false, false));
    let placed = placed_session(&[untimed.clone()]);
    let filter = MessageFilter::session(&placed);
    assert_eq!(filter.span, None);
    assert_eq!(filter.conversation, Some(conv(2)));
    assert!(filter.admits(&untimed, &placed));
}
