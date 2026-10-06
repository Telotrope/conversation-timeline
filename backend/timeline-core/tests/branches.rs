//! Pruning replaced branches (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4d): the kept
//! narrative is the path from the start to the most recent message; every
//! branch off it was replaced, and is measured by its words not repeated on
//! the kept path.

use serde_json::{json, Value};
use timeline_core::branches::{prune_replaced_branches, IMPORTANT_WORDS};
use timeline_core::model::{Conversation, MessageId};

const ROOT: &str = "00000000-0000-4000-8000-000000000000";

fn id(n: u32) -> String {
    format!("00000000-0000-0000-0000-{n:012}")
}

/// (id, parent id or 0 for the conversation's start, sender, minute, text)
fn conversation(messages: &[(u32, u32, &str, i64, &str)]) -> Conversation {
    let list: Vec<Value> = messages
        .iter()
        .map(|(n, parent, sender, minute, text)| {
            json!({
                "uuid": id(*n),
                "parent_message_uuid": if *parent == 0 { ROOT.to_string() } else { id(*parent) },
                "sender": sender,
                "created_at": chrono::DateTime::from_timestamp(1_700_000_000 + minute * 60, 0).unwrap(),
                "content": [{"type": "text", "text": text}],
            })
        })
        .collect();
    serde_json::from_value(json!({
        "uuid": id(999),
        "name": "c",
        "chat_messages": list,
    }))
    .unwrap()
}

fn ids(messages: &[timeline_core::ChatMessage]) -> Vec<String> {
    messages.iter().map(|m| m.uuid.to_string()).collect()
}

fn mid(n: u32) -> MessageId {
    MessageId(id(n).parse().unwrap())
}

#[test]
fn a_conversation_without_branches_is_kept_whole() {
    let c = conversation(&[
        (1, 0, "human", 0, "hi"),
        (2, 1, "assistant", 1, "hello"),
        (3, 2, "human", 2, "bye"),
    ]);
    let pruned = prune_replaced_branches(&c);
    assert_eq!(ids(&pruned.kept), vec![id(1), id(2), id(3)]);
    assert!(pruned.branches.is_empty());
}

/// The case in the plan: a message answered and continued, and a later
/// resend of it that went nowhere. "The newer reply wins" would keep the
/// dead end; the path to the latest message keeps the conversation.
#[test]
fn the_path_to_the_latest_message_is_kept_even_when_a_resend_is_newer() {
    let c = conversation(&[
        (1, 0, "human", 0, "How do I start?"),
        (2, 1, "assistant", 1, "Like this."),
        (3, 2, "human", 100, "Next question"),
        (4, 3, "assistant", 101, "Answer."),
        // The resend, eight minutes later, answered with nothing.
        (5, 0, "human", 8, "How do I start?"),
        (6, 5, "assistant", 9, ""),
    ]);
    let pruned = prune_replaced_branches(&c);
    assert_eq!(ids(&pruned.kept), vec![id(1), id(2), id(3), id(4)]);
    assert_eq!(pruned.branches.len(), 1);
    let branch = &pruned.branches[0];
    assert_eq!(ids(&branch.messages), vec![id(5), id(6)]);
    // It started at the conversation's start, where the kept path's own
    // first message took its place.
    assert_eq!(branch.replaced_by, Some(mid(1)));
    // The resend repeats a kept message, and the empty reply adds nothing.
    assert_eq!(branch.words_not_repeated, 0);
    assert!(!branch.is_important());
}

/// Text included in a kept message from the same sender counts as
/// repeated: a message cut off and sent again in full, or sent and then
/// extended. Runs of spaces and line breaks are collapsed first.
#[test]
fn a_cut_off_start_and_a_message_extended_later_count_as_repeated() {
    let c = conversation(&[
        (
            1,
            0,
            "human",
            0,
            "Please   help me with\nmy taxes this year",
        ),
        (2, 1, "assistant", 1, "Sure."),
        (3, 0, "human", -5, "Please help me"),
        (4, 2, "human", 3, "thanks"),
    ]);
    let pruned = prune_replaced_branches(&c);
    assert_eq!(pruned.branches.len(), 1);
    assert_eq!(pruned.branches[0].words_not_repeated, 0);
}

/// Text from the other sender isn't a repeat, however similar.
#[test]
fn only_the_same_senders_kept_messages_count_as_repeats() {
    let c = conversation(&[
        (1, 0, "human", 0, "start"),
        (2, 1, "assistant", 1, "two words here"),
        (3, 2, "human", 2, "later"),
        (4, 1, "human", -1, "two words"),
    ]);
    let branch = &prune_replaced_branches(&c).branches[0];
    assert_eq!(ids(&branch.messages), vec![id(4)]);
    assert_eq!(branch.replaced_by, Some(mid(2)));
    assert_eq!(branch.words_not_repeated, 2);
}

/// A branch of 100 words or more not repeated on the kept path is kept as
/// its own conversation (the user, Q5); 99 is not enough.
#[test]
fn a_branch_is_important_at_100_words_not_repeated() {
    let words = |n: usize| vec!["word"; n].join(" ");
    let ninety_nine = words(IMPORTANT_WORDS - 1);
    let hundred = words(IMPORTANT_WORDS);
    for (text, important) in [(&ninety_nine, false), (&hundred, true)] {
        let c = conversation(&[
            (1, 0, "human", 0, "start"),
            (2, 1, "assistant", 1, "ok"),
            (3, 2, "human", 5, "kept"),
            (4, 2, "human", 3, text),
        ]);
        let branch = &prune_replaced_branches(&c).branches[0];
        assert_eq!(branch.is_important(), important);
    }
}

/// A branch with branches of its own is one replaced branch, its messages
/// in time order; a branch answering the kept path's last message has no
/// replacement.
#[test]
fn a_branch_holds_all_its_descendants_and_one_off_the_end_has_no_replacement() {
    let c = conversation(&[
        (1, 0, "human", 0, "start"),
        (2, 1, "assistant", 1, "ok"),
        (3, 2, "human", 10, "kept"),
        (4, 2, "human", 3, "first try"),
        (5, 4, "assistant", 4, "reply a"),
        (6, 4, "assistant", 5, "reply b"),
        // Same time as the latest, listed first: the latest is the one
        // listed last, and this answers it.
        (8, 3, "assistant", 10, "late"),
    ]);
    let swapped = {
        let mut c = c.clone();
        let last = c.chat_messages.pop().unwrap();
        c.chat_messages.insert(2, last);
        c
    };
    let pruned = prune_replaced_branches(&swapped);
    assert_eq!(ids(&pruned.kept), vec![id(1), id(2), id(3)]);
    assert_eq!(pruned.branches.len(), 2);
    assert_eq!(ids(&pruned.branches[0].messages), vec![id(4), id(5), id(6)]);
    assert_eq!(pruned.branches[0].replaced_by, Some(mid(3)));
    assert_eq!(ids(&pruned.branches[1].messages), vec![id(8)]);
    assert_eq!(pruned.branches[1].replaced_by, None);
}

#[test]
fn links_that_cant_be_trusted_leave_the_conversation_whole() {
    // A parent that isn't in the conversation: a cut-down copy.
    let missing = conversation(&[(1, 0, "human", 0, "a"), (2, 77, "human", 1, "b")]);
    let pruned = prune_replaced_branches(&missing);
    assert_eq!(ids(&pruned.kept), vec![id(1), id(2)]);
    assert!(pruned.branches.is_empty());
    // No parent field at all: an older export, or a file made by hand.
    let mut unstated = conversation(&[(1, 0, "human", 0, "a"), (2, 1, "human", 1, "b")]);
    unstated.chat_messages[1]
        .extra
        .remove("parent_message_uuid");
    assert!(prune_replaced_branches(&unstated).branches.is_empty());
    assert_eq!(prune_replaced_branches(&unstated).kept.len(), 2);
    // A parent field that isn't an id counts as unstated.
    let mut garbled = conversation(&[(1, 0, "human", 0, "a")]);
    garbled.chat_messages[0]
        .extra
        .insert("parent_message_uuid".to_string(), json!(42));
    assert_eq!(prune_replaced_branches(&garbled).kept.len(), 1);
    // No messages at all.
    let empty = conversation(&[]);
    let pruned = prune_replaced_branches(&empty);
    assert!(pruned.kept.is_empty() && pruned.branches.is_empty());
}

/// Links that loop back on themselves can't send pruning round forever.
#[test]
fn a_loop_in_the_links_ends_the_path() {
    let c = conversation(&[(1, 2, "human", 0, "a"), (2, 1, "assistant", 1, "b")]);
    let pruned = prune_replaced_branches(&c);
    assert_eq!(ids(&pruned.kept), vec![id(1), id(2)]);
}
