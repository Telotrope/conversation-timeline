//! Black-box tests for `build_blocks`, calling only the crate's public API.

use timeline_core::{ChatMessage, ContentPiece, Conversation};

fn msg_at(sender: &str, ts: &str) -> ChatMessage {
    ChatMessage {
        uuid: format!("{sender}-{ts}"),
        text: String::new(),
        content: vec![ContentPiece {
            piece_type: "text".into(),
            text: String::new(),
            extra: Default::default(),
        }],
        sender: sender.into(),
        created_at: ts.into(),
        extra: Default::default(),
    }
}

fn conv(messages: Vec<ChatMessage>) -> Conversation {
    Conversation {
        uuid: "c".into(),
        name: "conv".into(),
        chat_messages: messages,
        extra: Default::default(),
    }
}

#[test]
fn messages_under_the_gap_threshold_stay_in_one_block() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("assistant", "2026-01-01T00:10:00Z"), // 600s gap
        msg_at("human", "2026-01-01T00:14:59Z"),     // 299s gap
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.blocks.len(), 1);
    assert_eq!(out.blocks[0].count, 3);
}

#[test]
fn gap_of_exactly_900_seconds_starts_a_new_block() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("human", "2026-01-01T00:15:00Z"), // exactly 900s
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(
        out.blocks.len(),
        2,
        "a 900s gap must split, per the >= comparison"
    );
}

#[test]
fn gap_of_899_seconds_does_not_split() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("human", "2026-01-01T00:14:59Z"), // 899s
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.blocks.len(), 1);
}

#[test]
fn a_session_spanning_local_midnight_is_one_block_since_no_day_bucketing_happens_here() {
    // A deliberate behavior difference from the original buildBlocks — see
    // the module doc in src/sessions.rs. Two messages either side of UTC
    // midnight, 5 minutes apart, must stay one block.
    let c = conv(vec![
        msg_at("human", "2026-01-01T23:58:00Z"),
        msg_at("human", "2026-01-02T00:03:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.blocks.len(), 1);
}

#[test]
fn assistant_messages_count_toward_gap_detection_too() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("assistant", "2026-01-01T00:20:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    // A 20-minute gap, even though only the *second* message is an
    // assistant reply, must still split — sender doesn't matter for gaps.
    assert_eq!(out.blocks.len(), 2);
}

#[test]
fn unsorted_input_is_sorted_before_gap_detection() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:10:00Z"),
        msg_at("human", "2026-01-01T00:00:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.blocks.len(), 1);
    assert_eq!(
        out.blocks[0].start.to_rfc3339(),
        "2026-01-01T00:00:00+00:00"
    );
}

#[test]
fn conversation_with_no_messages_produces_no_blocks() {
    let c = conv(vec![]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert!(out.blocks.is_empty());
}

#[test]
fn invalid_timestamps_are_skipped_but_counted_not_silently_dropped() {
    let c = conv(vec![
        msg_at("human", "not-a-real-timestamp"),
        msg_at("human", "2026-01-01T00:00:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.skipped_invalid_timestamps, 1);
    assert_eq!(out.blocks.len(), 1);
    assert_eq!(out.blocks[0].count, 1);
}

#[test]
fn multiple_conversations_are_kept_independent() {
    let c0 = conv(vec![msg_at("human", "2026-01-01T00:00:00Z")]);
    let c1 = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("human", "2026-01-01T01:00:00Z"),
    ]);
    let out = timeline_core::build_blocks(&[c0, c1]);
    assert_eq!(out.blocks.iter().filter(|b| b.conv == 0).count(), 1);
    assert_eq!(out.blocks.iter().filter(|b| b.conv == 1).count(), 2);
}

/// Snapshot of a synthetic multi-day, multi-gap sequence, pinning the exact
/// session boundaries the >=15-minute gap rule produces so a future change
/// to the splitting logic shows up as a reviewable diff.
#[test]
fn snapshot_of_a_multi_day_multi_gap_sequence() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T09:00:00Z"),
        msg_at("assistant", "2026-01-01T09:05:00Z"),
        msg_at("human", "2026-01-01T09:07:00Z"),
        // 20-minute gap -> new block, still day 1.
        msg_at("human", "2026-01-01T09:27:00Z"),
        // Large gap spanning into day 2 -> new block.
        msg_at("human", "2026-01-02T10:00:00Z"),
        msg_at("assistant", "2026-01-02T10:02:00Z"),
        // Exactly a 900s gap -> new block.
        msg_at("human", "2026-01-02T10:17:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    insta::assert_debug_snapshot!(out.blocks);
}

#[test]
fn duration_and_count_are_reported_per_block() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("assistant", "2026-01-01T00:05:00Z"),
        msg_at("human", "2026-01-01T00:09:00Z"),
    ]);
    let out = timeline_core::build_blocks(std::slice::from_ref(&c));
    assert_eq!(out.blocks.len(), 1);
    assert_eq!(out.blocks[0].duration_sec, 540);
    assert_eq!(out.blocks[0].count, 3);
}
