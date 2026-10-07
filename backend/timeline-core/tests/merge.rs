//! What a later file adds to a stored conversation (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4d, §4e, §10b;
//! screen-flow plan Q18): messages timed outside the stored range, by time
//! alone; and a revived branch replacing what the stored path held after its
//! branch point (C12).

use serde_json::{json, Value};
use timeline_core::conversation_metadata::ConversationSpan;
use timeline_core::keep::{branch_conversation_id, keep_conversation, Kept};
use timeline_core::merge::plan_merge;
use timeline_core::model::{Conversation, ConversationId, ConversationName, MessageId};
use timeline_core::stored_message::{Entry, EntryKey, Position};

const ROOT: &str = "00000000-0000-4000-8000-000000000000";
const CONV: &str = "cccccccc-0000-4000-8000-000000000001";

fn id(n: u32) -> String {
    format!("00000000-0000-0000-0000-{n:012}")
}

fn mid(n: u32) -> MessageId {
    MessageId(id(n).parse().unwrap())
}

fn time(minute: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0).unwrap()
}

/// (id, parent or 0 for the start, sender, minute or None for unknown, text)
fn file(messages: &[(u32, u32, &str, Option<i64>, &str)]) -> Kept {
    let list: Vec<Value> = messages
        .iter()
        .map(|(n, parent, sender, minute, text)| {
            let mut m = json!({
                "uuid": id(*n),
                "parent_message_uuid": if *parent == 0 { ROOT.to_string() } else { id(*parent) },
                "sender": sender,
                "content": [{"type": "text", "text": text}],
            });
            if let Some(minute) = minute {
                m["created_at"] = json!(time(*minute));
            }
            m
        })
        .collect();
    let conversation: Conversation =
        serde_json::from_value(json!({"uuid": CONV, "name": "Plans", "chat_messages": list}))
            .unwrap();
    keep_conversation(&conversation, true).unwrap()
}

fn span(from: i64, to: i64) -> ConversationSpan {
    ConversationSpan::new(time(from).fixed_offset(), time(to).fixed_offset()).unwrap()
}

fn added_ids(plan: &timeline_core::merge::MergePlan) -> Vec<MessageId> {
    plan.add
        .iter()
        .filter_map(Entry::as_message)
        .map(|m| m.key.id)
        .collect()
}

fn name() -> ConversationName {
    ConversationName("Plans".to_string())
}

#[test]
fn a_file_holding_nothing_new_changes_nothing() {
    let stored = file(&[
        (1, 0, "human", Some(0), "a"),
        (2, 1, "assistant", Some(1), "b"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(0, 1)), &name(), &stored);
    assert!(plan.changes_nothing());
    assert_eq!(plan, Default::default());
}

/// The user's rule (Q18): only messages timed outside the stored range are
/// added, compared by time alone; one inside it is not, even if new. A
/// message of unknown time can't be outside the range: never added, counted
/// as skipped (§4e).
#[test]
fn a_later_file_adds_messages_outside_the_stored_range_by_time_alone() {
    let stored = file(&[
        (1, 0, "human", Some(10), "a"),
        (2, 1, "assistant", Some(20), "b"),
    ]);
    let later = file(&[
        (1, 0, "human", Some(10), "a"),
        (5, 1, "human", Some(15), "inside, new"),
        (2, 5, "assistant", Some(20), "b"),
        (3, 2, "human", Some(30), "after"),
        (6, 3, "human", None, "unknown"),
        (4, 6, "assistant", Some(31), "reply"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(10, 20)), &name(), &later);
    assert_eq!(added_ids(&plan), vec![mid(3), mid(4)]);
    assert_eq!(plan.untimed_skipped, 1);
    assert!(plan.remove.is_empty() && plan.moved.is_none());
}

#[test]
fn a_stored_conversation_without_times_takes_every_timed_message() {
    let stored = file(&[]);
    let later = file(&[
        (1, 0, "human", Some(10), "a"),
        (2, 1, "assistant", Some(11), "b"),
    ]);
    let plan = plan_merge(&stored.main.entries, None, &name(), &later);
    assert_eq!(added_ids(&plan), vec![mid(1), mid(2)]);
}

/// A note, its important branch and the files of added messages come with
/// what is added.
#[test]
fn notes_branches_and_files_of_added_messages_come_with_them() {
    let long = vec!["word"; 120].join(" ");
    let stored = file(&[(1, 0, "human", Some(0), "a")]);
    let mut later = file(&[
        (1, 0, "human", Some(0), "a"),
        (2, 1, "assistant", Some(1), "b"),
        (3, 2, "human", Some(2), &long),
        (4, 2, "human", Some(3), "kept"),
    ]);
    later.main.files.push(timeline_core::keep::KeptFile {
        message_id: mid(4),
        number: 0,
        text: "a file".to_string(),
    });
    later.main.files.push(timeline_core::keep::KeptFile {
        message_id: mid(1),
        number: 0,
        text: "of a message not added".to_string(),
    });
    let plan = plan_merge(&stored.main.entries, Some(&span(0, 0)), &name(), &later);
    assert_eq!(added_ids(&plan), vec![mid(2), mid(4)]);
    let notes: Vec<_> = plan
        .add
        .iter()
        .filter(|e| e.as_message().is_none())
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(plan.branches.len(), 1);
    assert_eq!(plan.files.len(), 1);
    assert_eq!(plan.files[0].message_id, mid(4));
}

/// C12: the stored path ran start → 2 → 3 → 4, and the later file's newest
/// messages continue a branch off 2 that the first upload pruned. That
/// branch is now the path: what the stored path held after 2 is replaced
/// (a note where it began), and the branch's messages are added from the
/// file, even those inside the stored range.
#[test]
fn a_revived_branch_replaces_the_stored_path_after_its_branch_point() {
    let stored = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
        (4, 3, "assistant", Some(6), "reply"),
    ]);
    let later = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
        (4, 3, "assistant", Some(6), "reply"),
        (7, 2, "human", Some(3), "the branch"),
        (8, 7, "assistant", Some(4), "its reply"),
        (9, 8, "human", Some(50), "newest"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(0, 6)), &name(), &later);
    // Stored keys carry each message's place in the first file (plan §12.3).
    let stored_key = |n: u32, position: i64, minute: i64| EntryKey {
        conversation_id: ConversationId(CONV.parse().unwrap()),
        position: Position(position),
        at: time(minute),
        id: mid(n),
    };
    assert_eq!(plan.remove, vec![stored_key(3, 2, 5), stored_key(4, 3, 6)]);
    assert_eq!(added_ids(&plan), vec![mid(7), mid(8), mid(9)]);
    let note = plan
        .add
        .iter()
        .find_map(|e| match e {
            Entry::Note(n) => Some(n),
            Entry::Message(_) => None,
        })
        .unwrap();
    // The note stands where the file lists the replaced branch, numbered
    // with the new path after every stored position (plan §12.3).
    assert_eq!(note.key, stored_key(3, 4, 5));
    assert_eq!(
        plan.add
            .iter()
            .map(|e| (e.key().id, e.key().position.0))
            .collect::<Vec<_>>(),
        vec![(mid(3), 4), (mid(7), 5), (mid(8), 6), (mid(9), 7)]
    );
    assert_eq!(note.last_at, time(6));
    assert_eq!(note.messages, 2);
    // "first try" is on no kept message of the file; "reply" is included in
    // "its reply", so it counts as repeated.
    assert_eq!(note.words_not_repeated, 2);
    assert_eq!(note.replaced_by, Some(mid(7)));
    assert_eq!(note.kept_as, None);
    assert!(plan.moved.is_none());
}

/// A revived branch whose replaced messages hold 100 words not repeated on
/// the new path moves them into a conversation of their own.
#[test]
fn an_important_replaced_path_moves_to_a_conversation_of_its_own() {
    let long = vec!["word"; 150].join(" ");
    let stored = file(&[
        (1, 0, "human", Some(0), "start"),
        (3, 1, "human", Some(5), &long),
        (4, 3, "assistant", Some(6), "reply"),
    ]);
    let later = file(&[
        (1, 0, "human", Some(0), "start"),
        (7, 1, "human", Some(3), "the branch"),
        (9, 7, "human", Some(50), "newest"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(0, 6)), &name(), &later);
    let moved = plan.moved.as_ref().unwrap();
    let new_id = branch_conversation_id(ConversationId(CONV.parse().unwrap()), mid(3));
    assert_eq!(moved.conversation_id, new_id);
    assert_eq!(
        moved.name.0,
        "Plans: earlier branch from 2023-11-14 22:18 UTC"
    );
    assert_eq!(moved.entries.len(), 2);
    assert!(moved
        .entries
        .iter()
        .all(|e| e.key().conversation_id == new_id));
    let note = plan.add.iter().find(|e| e.as_message().is_none()).unwrap();
    let Entry::Note(note) = note else {
        unreachable!()
    };
    assert_eq!(note.kept_as, Some(new_id));
    assert!(note.words_not_repeated >= 150);
}

/// Unknown times sort first, at the zero date; a replaced path that starts
/// with a message of unknown time is named so.
#[test]
fn a_replaced_message_of_unknown_time_is_named_so() {
    let long = vec!["word"; 150].join(" ");
    let stored = file(&[
        (1, 0, "human", None, "start"),
        (3, 1, "human", None, &long),
        (4, 3, "assistant", Some(6), "reply"),
    ]);
    let later = file(&[
        (1, 0, "human", None, "start"),
        (7, 1, "human", Some(3), "the branch"),
        (9, 7, "human", Some(50), "newest"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(6, 6)), &name(), &later);
    assert_eq!(plan.remove.len(), 2, "{plan:?}");
    assert_eq!(
        plan.moved.unwrap().name.0,
        "Plans: earlier branch from an unknown time"
    );
}

/// Not a revival: the first new message continues from the stored path's
/// last message (an ordinary extension); its parent isn't stored; it starts
/// the conversation; or the file's newest message is older than the stored
/// newest (the file holds an older branch, which stays pruned).
#[test]
fn only_a_newer_branch_off_a_stored_message_is_a_revival() {
    let stored = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
    ]);
    let s = span(0, 5);
    let extension = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
        (4, 3, "assistant", Some(9), "more"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&s), &name(), &extension);
    assert!(plan.remove.is_empty());
    assert_eq!(added_ids(&plan), vec![mid(4)]);

    let older_branch = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (7, 2, "human", Some(2), "an older try"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&s), &name(), &older_branch);
    assert!(plan.changes_nothing(), "{plan:?}");

    let unknown_parent = file(&[(8, 77, "human", Some(60), "orphan")]);
    let plan = plan_merge(&stored.main.entries, Some(&s), &name(), &unknown_parent);
    assert!(plan.remove.is_empty());
    assert_eq!(added_ids(&plan), vec![mid(8)]);

    let new_start = file(&[(8, 0, "human", Some(60), "new start")]);
    let plan = plan_merge(&stored.main.entries, Some(&s), &name(), &new_start);
    assert!(plan.remove.is_empty());

    let mut unstated = file(&[
        (1, 0, "human", Some(0), "start"),
        (8, 1, "human", Some(60), "next"),
    ]);
    for message in &mut unstated.main.kept_path {
        message.extra.remove("parent_message_uuid");
    }
    let plan = plan_merge(&stored.main.entries, Some(&s), &name(), &unstated);
    assert!(plan.remove.is_empty());
}

/// When everything the stored path held after the branch point is also on
/// the file's path, nothing is replaced: the time rule decides what is
/// added.
#[test]
fn a_revival_replacing_nothing_only_adds() {
    let stored = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
    ]);
    // 3 is still on the file's path, but the new message 7 answers 2.
    let later = file(&[
        (1, 0, "human", Some(0), "start"),
        (2, 1, "assistant", Some(1), "ok"),
        (3, 2, "human", Some(5), "first try"),
    ]);
    let mut later = later;
    let extra: timeline_core::model::ChatMessage = serde_json::from_value(json!({
        "uuid": id(7), "parent_message_uuid": id(2), "sender": "human",
        "created_at": time(60), "content": [{"type": "text", "text": "late"}],
    }))
    .unwrap();
    let kept_extra = keep_conversation(
        &serde_json::from_value(
            json!({"uuid": CONV, "name": "Plans", "chat_messages": [extra.clone()]}),
        )
        .unwrap(),
        true,
    )
    .unwrap();
    later.main.kept_path.push(extra);
    later.main.entries.extend(kept_extra.main.entries);
    let plan = plan_merge(&stored.main.entries, Some(&span(0, 5)), &name(), &later);
    assert!(plan.remove.is_empty(), "{plan:?}");
    assert_eq!(added_ids(&plan), vec![mid(7)]);
}

/// Plan §12.3: what a later file adds before the stored range is numbered
/// below the first stored position, and what it adds after it above the
/// last, each in the file's order.
#[test]
fn a_later_files_additions_are_numbered_before_or_after_the_stored_ones() {
    let stored = file(&[
        (1, 0, "human", Some(10), "a"),
        (2, 1, "assistant", Some(11), "b"),
    ]);
    let later = file(&[
        (5, 0, "human", Some(0), "earlier"),
        (6, 5, "assistant", Some(1), "earlier reply"),
        (1, 6, "human", Some(10), "a"),
        (2, 1, "assistant", Some(11), "b"),
        (7, 2, "human", Some(20), "later"),
    ]);
    let plan = plan_merge(&stored.main.entries, Some(&span(10, 11)), &name(), &later);
    let positions: Vec<(MessageId, i64)> = plan
        .add
        .iter()
        .map(|e| (e.key().id, e.key().position.0))
        .collect();
    assert_eq!(positions, vec![(mid(5), -2), (mid(6), -1), (mid(7), 2)]);
}
