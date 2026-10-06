//! What is kept of a message (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §2–§4d).

#[path = "support/entries.rs"]
mod entries;

use entries::*;
use timeline_core::labels::FileName;
use timeline_core::model::Sender;
use timeline_core::stored_message::{
    CitedAddress, Entry, FileContents, FileKind, FileRef, Piece, StoredMessage,
};
use timeline_core::{MessageTime, UNKNOWN_TIME};

fn file(number: usize, name: &str) -> FileRef {
    FileRef {
        number,
        name: FileName::parse(name).unwrap(),
        kind: FileKind::Markdown,
        contents: FileContents::Stored,
        may_have_changed_later: false,
    }
}

/// Only `http` and `https` addresses become links; anything else is kept as
/// text, so a stored citation can never make the page open a script.
#[test]
fn only_web_addresses_become_links() {
    assert_eq!(
        CitedAddress::parse("https://example.com/a"),
        CitedAddress::Web("https://example.com/a".to_string())
    );
    assert_eq!(
        CitedAddress::parse("  HTTP://Example.com "),
        CitedAddress::Web("HTTP://Example.com".to_string())
    );
    for other in ["javascript:alert(1)", "data:text/html,x", "ftp://x", ""] {
        assert_eq!(
            CitedAddress::parse(other),
            CitedAddress::Other(other.to_string())
        );
    }
}

/// Replaces `export-format.test.js` L10 ("extractMessageText joins text
/// pieces and skips other kinds"): a stored message's text joins its text
/// pieces and skips file marks.
#[test]
fn a_messages_text_joins_its_text_pieces_and_skips_files() {
    let message = StoredMessage {
        key: key(1, minute(0), 1),
        parent: None,
        sender: Sender::Assistant,
        pieces: vec![
            Piece::Text {
                text: "Here it is.".to_string(),
                citations: Vec::new(),
            },
            Piece::File {
                file: file(0, "plan.md"),
            },
            Piece::Text {
                text: " And more.".to_string(),
                citations: Vec::new(),
            },
        ],
        attachments: vec![file(1, "notes.txt")],
        flags: None,
    };
    assert_eq!(message.text(), "Here it is. And more.");
    let names: Vec<&str> = message.files().map(|f| f.name.as_str()).collect();
    assert_eq!(names, vec!["plan.md", "notes.txt"]);
}

#[test]
fn an_entry_knows_its_key_its_activity_and_whose_flags_it_carries() {
    let mine = yours(key(1, minute(0), 1), auto(true, false, false));
    let reply = claudes(key(1, minute(1), 2));
    let a_note = note(key(1, minute(2), 3), minute(9));
    assert_eq!(mine.key(), key(1, minute(0), 1));
    assert_eq!(mine.activity(), (minute(0), minute(0)));
    assert_eq!(a_note.activity(), (minute(2), minute(9)));
    assert_eq!(a_note.key(), key(1, minute(2), 3));
    assert!(mine.as_message().is_some());
    assert!(a_note.as_message().is_none());
    assert_eq!(mine.your_flags(), Some(&auto(true, false, false)));
    assert_eq!(reply.your_flags(), None);
    assert_eq!(a_note.your_flags(), None);
}

#[test]
fn a_key_at_the_zero_date_reads_as_unknown_time() {
    assert_eq!(key(1, UNKNOWN_TIME, 1).time(), MessageTime::Unknown);
    assert_eq!(key(1, minute(3), 1).time(), MessageTime::Known(minute(3)));
}

#[test]
fn entries_serialize_with_their_kind() {
    let a_note = note(key(1, minute(2), 3), minute(9));
    let json = serde_json::to_value(&a_note).unwrap();
    assert_eq!(json["entry"], "note");
    let back: Entry = serde_json::from_value(json).unwrap();
    assert_eq!(back, a_note);
}
