//! Splits each conversation's messages into session blocks wherever the gap
//! since the previous message is 15 minutes (900s) or more.
//!
//! This is a **deliberate partial port** of `buildBlocks` at
//! [timeline.html:65229-65264](../../../timeline.html#L65229), not a literal
//! one. The JS version fuses two things with different timezone sensitivity:
//! gap-based splitting (timezone-agnostic — a delta between two instants) and
//! which *local calendar day* a session renders under (timezone-sensitive,
//! and the source of a real bug once already, when that part was computed
//! server-side in UTC — see
//! [timeline-project-decisions.md:371](../../../timeline-project-decisions.md#L371)).
//! Per the migration plan §4.3, only the gap-based half moves to the backend;
//! day-bucketing stays a client-side rendering concern, operating on the UTC
//! boundaries this function returns. So this function takes UTC timestamps in
//! and returns UTC session boundaries out, with **no day bucketing at all** —
//! a session spanning local midnight is one block here, same as it would be
//! for any other sub-15-minute gap.

use chrono::{DateTime, Utc};

use crate::model::Conversation;

pub const GAP_THRESHOLD_SEC: i64 = 15 * 60;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionBlock {
    /// Index of the conversation in the input slice this block belongs to.
    pub conv: usize,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub duration_sec: i64,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuildBlocksOutput {
    pub blocks: Vec<SessionBlock>,
    /// Messages with a `created_at` that didn't parse as RFC 3339 — excluded
    /// from block-building but counted rather than silently dropped, so a
    /// caller can decide whether that's acceptable for their data.
    pub skipped_invalid_timestamps: usize,
}

/// Builds session blocks across every conversation in `conversations`. Every
/// message with a parseable `created_at` counts toward gap detection,
/// regardless of sender — an assistant reply keeps a session "warm" just as
/// much as a human message does, matching the original's `MESSAGES` list.
pub fn build_blocks(conversations: &[Conversation]) -> BuildBlocksOutput {
    let mut skipped = 0usize;
    let mut by_conv: Vec<Vec<DateTime<Utc>>> = vec![Vec::new(); conversations.len()];

    for (conv_idx, conv) in conversations.iter().enumerate() {
        for m in &conv.chat_messages {
            match DateTime::parse_from_rfc3339(&m.created_at) {
                Ok(ts) => by_conv[conv_idx].push(ts.with_timezone(&Utc)),
                Err(_) => skipped += 1,
            }
        }
    }

    let mut blocks = Vec::new();
    for (conv_idx, mut timestamps) in by_conv.into_iter().enumerate() {
        if timestamps.is_empty() {
            continue;
        }
        timestamps.sort();

        let mut run_start = 0usize;
        for i in 1..=timestamps.len() {
            let gap_sec = if i < timestamps.len() {
                (timestamps[i] - timestamps[i - 1]).num_seconds()
            } else {
                i64::MAX
            };
            if gap_sec >= GAP_THRESHOLD_SEC || i == timestamps.len() {
                let run = &timestamps[run_start..i];
                let start = run[0];
                let end = run[run.len() - 1];
                blocks.push(SessionBlock {
                    conv: conv_idx,
                    start,
                    end,
                    duration_sec: (end - start).num_seconds(),
                    count: run.len(),
                });
                run_start = i;
            }
        }
    }

    BuildBlocksOutput {
        blocks,
        skipped_invalid_timestamps: skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ChatMessage, ContentPiece};

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
        let out = build_blocks(std::slice::from_ref(&c));
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(out.blocks[0].count, 3);
    }

    #[test]
    fn gap_of_exactly_900_seconds_starts_a_new_block() {
        let c = conv(vec![
            msg_at("human", "2026-01-01T00:00:00Z"),
            msg_at("human", "2026-01-01T00:15:00Z"), // exactly 900s
        ]);
        let out = build_blocks(std::slice::from_ref(&c));
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
        let out = build_blocks(std::slice::from_ref(&c));
        assert_eq!(out.blocks.len(), 1);
    }

    #[test]
    fn a_session_spanning_local_midnight_is_one_block_since_no_day_bucketing_happens_here() {
        // A deliberate behavior difference from the original buildBlocks —
        // see the module doc. Two messages either side of UTC midnight, 5
        // minutes apart, must stay one block.
        let c = conv(vec![
            msg_at("human", "2026-01-01T23:58:00Z"),
            msg_at("human", "2026-01-02T00:03:00Z"),
        ]);
        let out = build_blocks(std::slice::from_ref(&c));
        assert_eq!(out.blocks.len(), 1);
    }

    #[test]
    fn assistant_messages_count_toward_gap_detection_too() {
        let c = conv(vec![
            msg_at("human", "2026-01-01T00:00:00Z"),
            msg_at("assistant", "2026-01-01T00:20:00Z"),
        ]);
        let out = build_blocks(std::slice::from_ref(&c));
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
        let out = build_blocks(std::slice::from_ref(&c));
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(
            out.blocks[0].start.to_rfc3339(),
            "2026-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn conversation_with_no_messages_produces_no_blocks() {
        let c = conv(vec![]);
        let out = build_blocks(std::slice::from_ref(&c));
        assert!(out.blocks.is_empty());
    }

    #[test]
    fn invalid_timestamps_are_skipped_but_counted_not_silently_dropped() {
        let c = conv(vec![
            msg_at("human", "not-a-real-timestamp"),
            msg_at("human", "2026-01-01T00:00:00Z"),
        ]);
        let out = build_blocks(std::slice::from_ref(&c));
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
        let out = build_blocks(&[c0, c1]);
        assert_eq!(out.blocks.iter().filter(|b| b.conv == 0).count(), 1);
        assert_eq!(out.blocks.iter().filter(|b| b.conv == 1).count(), 2);
    }

    /// Snapshot of a synthetic multi-day, multi-gap sequence, pinning the
    /// exact session boundaries the >=15-minute gap rule produces so a
    /// future change to the splitting logic shows up as a reviewable diff.
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
        let out = build_blocks(std::slice::from_ref(&c));
        insta::assert_debug_snapshot!(out.blocks);
    }

    #[test]
    fn duration_and_count_are_reported_per_block() {
        let c = conv(vec![
            msg_at("human", "2026-01-01T00:00:00Z"),
            msg_at("assistant", "2026-01-01T00:05:00Z"),
            msg_at("human", "2026-01-01T00:09:00Z"),
        ]);
        let out = build_blocks(std::slice::from_ref(&c));
        assert_eq!(out.blocks.len(), 1);
        assert_eq!(out.blocks[0].duration_sec, 540);
        assert_eq!(out.blocks[0].count, 3);
    }
}
