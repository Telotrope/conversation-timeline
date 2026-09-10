//! Black-box tests for `dedup_chat_messages`/`dedup_conversations`, calling
//! only the crate's public API — see `docs/plans/2026-09-09-rust-aws-backend-migration.md`
//! for why this crate's tests live in `tests/` rather than as unit tests
//! colocated with the implementation.

use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};
use timeline_core::{
    dedup_chat_messages, dedup_conversations, extract_text, ChatMessage, ContentPiece,
    Conversation, ConversationId, ConversationName, MessageId, PieceType, Sender,
};

/// Deterministic, collision-free-enough-for-tests UUID from any seed string
/// — the exact value never matters here, only that distinct seeds produce
/// distinct, validly-formed ids.
fn uuid_from(seed: &str) -> uuid::Uuid {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    uuid::Uuid::from_u128(hasher.finish() as u128)
}

/// Turns a short tag like `"t1"` or `"ts003"` into a real, order-preserving
/// timestamp — the tests only care about relative ordering between tags,
/// never the actual instant.
fn ts(tag: &str) -> DateTime<Utc> {
    let digits: String = tag.chars().filter(char::is_ascii_digit).collect();
    let offset_secs: i64 = digits.parse().unwrap_or(0);
    DateTime::from_timestamp(1_700_000_000 + offset_secs, 0).expect("valid timestamp")
}

fn msg(sender: &str, text: &str, tag: &str) -> ChatMessage {
    ChatMessage {
        uuid: MessageId(uuid_from(&format!("{sender}-{tag}"))),
        text: text.to_string(),
        content: vec![ContentPiece {
            piece_type: PieceType::Text,
            text: text.to_string(),
            extra: Default::default(),
        }],
        sender: Sender::from(sender.to_string()),
        created_at: ts(tag),
        extra: Default::default(),
    }
}

fn human(text: &str, tag: &str) -> ChatMessage {
    msg("human", text, tag)
}
fn assistant(text: &str, tag: &str) -> ChatMessage {
    msg("assistant", text, tag)
}

fn texts(msgs: &[ChatMessage]) -> Vec<String> {
    msgs.iter().map(extract_text).collect()
}

/// `extract_text` sums only `type: "text"` pieces, exactly like the
/// original `extractMessageText` — a non-text piece (e.g. a tool_use block)
/// never contributes to the dedup comparison.
#[test]
fn extract_text_ignores_non_text_pieces() {
    let m = ChatMessage {
        uuid: MessageId(uuid_from("u")),
        text: "ignored top-level field".into(),
        content: vec![
            ContentPiece {
                piece_type: PieceType::Other("tool_use".into()),
                text: "should not appear".into(),
                extra: Default::default(),
            },
            ContentPiece {
                piece_type: PieceType::Text,
                text: "hello ".into(),
                extra: Default::default(),
            },
            ContentPiece {
                piece_type: PieceType::Text,
                text: "world".into(),
                extra: Default::default(),
            },
        ],
        sender: Sender::Human,
        created_at: ts("t0"),
        extra: Default::default(),
    };
    assert_eq!(extract_text(&m), "hello world");
}

/// Case 1 — simple chain: one resend, no reply in between.
#[test]
fn simple_chain_keeps_last() {
    let msgs = vec![human("hi", "t0"), human("hi", "t1"), assistant("hey", "t2")];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["hi", "hey"]);
    assert_eq!(
        out[0].created_at,
        ts("t1"),
        "kept human message must be the later resend"
    );
}

/// Case 2 — a stray reply to an early attempt: human, assistant (to the
/// failed attempt), human (identical resend), assistant (real reply) — the
/// stray assistant reply is dropped along with the first human attempt.
#[test]
fn stray_reply_to_early_attempt_is_dropped() {
    let msgs = vec![
        human("please help", "t0"),
        assistant("stray/failed reply", "t1"),
        human("please help", "t2"),
        assistant("real reply", "t3"),
    ];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["please help", "real reply"]);
    assert_eq!(out[0].created_at, ts("t2"));
}

/// Case 3 — no duplicates: a plain alternating conversation is untouched.
#[test]
fn no_duplicates_is_a_no_op() {
    let msgs = vec![
        human("first question", "t0"),
        assistant("first answer", "t1"),
        human("second question", "t2"),
        assistant("second answer", "t3"),
    ];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(out, msgs);
}

/// Case 4 — mid-conversation duplicates: the run isn't at the very start.
#[test]
fn mid_conversation_duplicates_are_collapsed() {
    let msgs = vec![
        human("q1", "t0"),
        assistant("a1", "t1"),
        human("q2", "t2"),
        human("q2", "t3"),
        assistant("a2", "t4"),
        human("q3", "t5"),
        assistant("a3", "t6"),
    ];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["q1", "a1", "q2", "a2", "q3", "a3"]);
    assert_eq!(out[2].created_at, ts("t3"));
}

/// Case 5 — a 5-way chain with a stray reply sandwiched partway through:
/// every human message is textually identical; only the final one
/// survives, along with everything from the last occurrence onward.
#[test]
fn five_way_chain_with_stray_reply_keeps_only_the_last() {
    let msgs = vec![
        human("retry me", "t0"),
        human("retry me", "t1"),
        assistant("stray reply mid-chain", "t2"),
        human("retry me", "t3"),
        human("retry me", "t4"),
        human("retry me", "t5"),
        assistant("finally, a real reply", "t6"),
    ];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["retry me", "finally, a real reply"]);
    assert_eq!(
        out[0].created_at,
        ts("t5"),
        "only the last of the 5 resends survives"
    );
}

/// Case 6 — two independent duplicate runs in the same conversation, each
/// resolved on its own.
#[test]
fn two_separate_duplicate_runs() {
    let msgs = vec![
        human("a", "t0"),
        human("a", "t1"),
        assistant("reply a", "t2"),
        human("b", "t3"),
        human("b", "t4"),
        assistant("reply b", "t5"),
    ];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["a", "reply a", "b", "reply b"]);
}

#[test]
fn empty_conversation_is_untouched() {
    assert_eq!(dedup_chat_messages(&[]), Vec::<ChatMessage>::new());
}

/// A run broken by a genuinely different human message ends the scan — the
/// second, different human message starts its own fresh run.
#[test]
fn different_human_message_breaks_the_run() {
    let msgs = vec![human("a", "t0"), human("a", "t1"), human("different", "t2")];
    let out = dedup_chat_messages(&msgs);
    assert_eq!(texts(&out), vec!["a", "different"]);
    assert_eq!(out[0].created_at, ts("t1"));
}

#[test]
fn dedup_conversations_runs_on_every_conversation() {
    let mut convs = vec![
        Conversation {
            uuid: ConversationId(uuid_from("c0")),
            name: ConversationName("conv 0".into()),
            chat_messages: vec![human("a", "t0"), human("a", "t1")],
            extra: Default::default(),
        },
        Conversation {
            uuid: ConversationId(uuid_from("c1")),
            name: ConversationName("conv 1".into()),
            chat_messages: vec![human("b", "t0"), assistant("reply", "t1")],
            extra: Default::default(),
        },
    ];
    dedup_conversations(&mut convs);
    assert_eq!(convs[0].chat_messages.len(), 1);
    assert_eq!(convs[1].chat_messages.len(), 2);
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_message_seq() -> impl Strategy<Value = Vec<ChatMessage>> {
        prop::collection::vec(
            (prop_oneof![Just("human"), Just("assistant")], 0usize..4),
            0..15,
        )
        .prop_map(|seq| {
            seq.into_iter()
                .enumerate()
                .map(|(idx, (sender, text_idx))| {
                    msg(sender, &format!("text-{text_idx}"), &format!("ts{idx:03}"))
                })
                .collect()
        })
    }

    proptest! {
        /// Dedup can only remove messages, never add or duplicate them.
        #[test]
        fn never_increases_message_count(msgs in arb_message_seq()) {
            let out = dedup_chat_messages(&msgs);
            prop_assert!(out.len() <= msgs.len());
        }

        /// After dedup, no two adjacent human messages (in the surviving
        /// human subsequence) have identical text — that's the whole point
        /// of the run-collapsing rule.
        #[test]
        fn no_adjacent_identical_human_messages_survive(msgs in arb_message_seq()) {
            let out = dedup_chat_messages(&msgs);
            let human_texts: Vec<String> = out.iter().filter(|m| m.sender == Sender::Human).map(extract_text).collect();
            for pair in human_texts.windows(2) {
                prop_assert_ne!(&pair[0], &pair[1]);
            }
        }

        /// Every surviving message was present, unchanged, in the input —
        /// dedup only removes, it never rewrites a message.
        #[test]
        fn every_surviving_message_is_identical_to_some_input_message(msgs in arb_message_seq()) {
            let out = dedup_chat_messages(&msgs);
            for m in &out {
                prop_assert!(msgs.contains(m));
            }
        }
    }
}
