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
//!
//! Every message's `created_at` is already a validated `DateTime<Utc>` by
//! the time it reaches this function — parsing happens once, at
//! deserialization ([`crate::model::ChatMessage`]), not here. An earlier
//! version of this function parsed `created_at` itself (from a raw string)
//! and skipped-and-counted whatever didn't parse; that handling moved to the
//! parse boundary, so the "what if it's invalid" case this function used to
//! carry doesn't exist anymore — there's no `String` left to be invalid.

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

/// Builds session blocks across every conversation in `conversations`. Every
/// message counts toward gap detection, regardless of sender — an assistant
/// reply keeps a session "warm" just as much as a human message does,
/// matching the original's `MESSAGES` list.
pub fn build_blocks(conversations: &[Conversation]) -> Vec<SessionBlock> {
    let mut by_conv: Vec<Vec<DateTime<Utc>>> = vec![Vec::new(); conversations.len()];

    for (conv_idx, conv) in conversations.iter().enumerate() {
        for m in &conv.chat_messages {
            by_conv[conv_idx].push(m.created_at);
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

    blocks
}
