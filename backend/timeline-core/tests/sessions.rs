//! Black-box tests for `build_blocks`, calling only the crate's public API.
//!
//! `created_at` is validated at deserialization time now (see
//! `crate::model::ChatMessage`), so there's no "what if it's invalid" case
//! left for `build_blocks` itself to handle — a `ChatMessage` literally
//! can't be constructed with an unparseable timestamp. The parse-time
//! rejection of a bad timestamp is tested in `tests/model.rs` instead.

use timeline_core::{
    build_blocks, ChatMessage, ContentPiece, Conversation, ConversationId, ConversationName,
    MessageId, PieceType, Sender,
};

fn msg_at(sender: &str, ts: &str) -> ChatMessage {
    ChatMessage {
        uuid: MessageId(uuid::Uuid::from_u128(0)),
        text: String::new(),
        content: vec![ContentPiece {
            piece_type: PieceType::Text,
            text: String::new(),
            extra: Default::default(),
        }],
        sender: Sender::from(sender.to_string()),
        created_at: ts
            .parse()
            .expect("test fixture timestamps are valid RFC 3339"),
        extra: Default::default(),
    }
}

fn conv(messages: Vec<ChatMessage>) -> Conversation {
    Conversation {
        uuid: ConversationId(uuid::Uuid::from_u128(0)),
        name: ConversationName("conv".into()),
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
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].count, 3);
}

#[test]
fn gap_of_exactly_900_seconds_starts_a_new_block() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("human", "2026-01-01T00:15:00Z"), // exactly 900s
    ]);
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(
        blocks.len(),
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
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(blocks.len(), 1);
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
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(blocks.len(), 1);
}

#[test]
fn assistant_messages_count_toward_gap_detection_too() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("assistant", "2026-01-01T00:20:00Z"),
    ]);
    let blocks = build_blocks(std::slice::from_ref(&c));
    // A 20-minute gap, even though only the *second* message is an
    // assistant reply, must still split — sender doesn't matter for gaps.
    assert_eq!(blocks.len(), 2);
}

#[test]
fn unsorted_input_is_sorted_before_gap_detection() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:10:00Z"),
        msg_at("human", "2026-01-01T00:00:00Z"),
    ]);
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].start.to_rfc3339(), "2026-01-01T00:00:00+00:00");
}

#[test]
fn conversation_with_no_messages_produces_no_blocks() {
    let c = conv(vec![]);
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert!(blocks.is_empty());
}

#[test]
fn multiple_conversations_are_kept_independent() {
    let c0 = conv(vec![msg_at("human", "2026-01-01T00:00:00Z")]);
    let c1 = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("human", "2026-01-01T01:00:00Z"),
    ]);
    let blocks = build_blocks(&[c0, c1]);
    assert_eq!(blocks.iter().filter(|b| b.conv == 0).count(), 1);
    assert_eq!(blocks.iter().filter(|b| b.conv == 1).count(), 2);
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
    let blocks = build_blocks(std::slice::from_ref(&c));
    insta::assert_debug_snapshot!(blocks);
}

#[test]
fn duration_and_count_are_reported_per_block() {
    let c = conv(vec![
        msg_at("human", "2026-01-01T00:00:00Z"),
        msg_at("assistant", "2026-01-01T00:05:00Z"),
        msg_at("human", "2026-01-01T00:09:00Z"),
    ]);
    let blocks = build_blocks(std::slice::from_ref(&c));
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].duration_sec, 540);
    assert_eq!(blocks[0].count, 3);
}
