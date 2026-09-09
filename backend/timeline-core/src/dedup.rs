//! Removes retried duplicate human messages: when a request silently fails to
//! get a response, resending it later produces two human messages with
//! identical text, adjacent in the *human* subsequence (assistant messages
//! may sit between them). For a run of n identical resends, keeps only the
//! last one (the one that actually got a reply) and drops requests 1..n-1
//! along with anything — of any sender — that occurred between the first and
//! last occurrence.
//!
//! Faithful port of `dedupChatMessages` at
//! [timeline.html:64906-64940](../../../timeline.html#L64906).

use crate::model::ChatMessage;

/// Concatenates every `content` piece of type `"text"`, matching
/// `extractMessageText` at [timeline.html:64891-64897](../../../timeline.html#L64891).
pub fn extract_text(message: &ChatMessage) -> String {
    let mut text = String::new();
    for piece in &message.content {
        if piece.piece_type == "text" {
            text.push_str(&piece.text);
        }
    }
    text
}

/// Deduplicates one conversation's messages in place order, returning the
/// kept subsequence. Port of `dedupChatMessages`.
pub fn dedup_chat_messages(chat_messages: &[ChatMessage]) -> Vec<ChatMessage> {
    let n = chat_messages.len();
    let mut result = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        let m = &chat_messages[i];
        if m.sender != "human" {
            result.push(m.clone());
            i += 1;
            continue;
        }
        let text = extract_text(m);
        let mut last_dup_index = i;
        let mut k = i + 1;
        while k < n {
            if chat_messages[k].sender == "human" {
                if extract_text(&chat_messages[k]) == text {
                    last_dup_index = k;
                    k += 1;
                } else {
                    break; // a genuinely different human message ends the run
                }
            } else {
                k += 1; // assistant message inside the run; dropped if before last_dup_index
            }
        }
        if last_dup_index == i {
            result.push(m.clone());
            i += 1;
        } else {
            i = last_dup_index; // jump straight to the final resend, dropping everything in between
        }
    }
    result
}

/// Deduplicates `chat_messages` on every conversation in place. Port of
/// `dedupConversations` at [timeline.html:64942-64947](../../../timeline.html#L64942).
pub fn dedup_conversations(conversations: &mut [crate::model::Conversation]) {
    for c in conversations.iter_mut() {
        c.chat_messages = dedup_chat_messages(&c.chat_messages);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContentPiece;

    fn msg(sender: &str, text: &str, ts: &str) -> ChatMessage {
        ChatMessage {
            uuid: format!("{sender}-{ts}"),
            text: text.to_string(),
            content: vec![ContentPiece {
                piece_type: "text".to_string(),
                text: text.to_string(),
                extra: Default::default(),
            }],
            sender: sender.to_string(),
            created_at: ts.to_string(),
            extra: Default::default(),
        }
    }

    fn human(text: &str, ts: &str) -> ChatMessage {
        msg("human", text, ts)
    }
    fn assistant(text: &str, ts: &str) -> ChatMessage {
        msg("assistant", text, ts)
    }

    fn texts(msgs: &[ChatMessage]) -> Vec<String> {
        msgs.iter().map(extract_text).collect()
    }

    /// extract_text sums only `type: "text"` pieces, exactly like
    /// `extractMessageText` — a non-text piece (e.g. a tool_use block) never
    /// contributes to the dedup comparison.
    #[test]
    fn extract_text_ignores_non_text_pieces() {
        let m = ChatMessage {
            uuid: "u".into(),
            text: "ignored top-level field".into(),
            content: vec![
                ContentPiece {
                    piece_type: "tool_use".into(),
                    text: "should not appear".into(),
                    extra: Default::default(),
                },
                ContentPiece {
                    piece_type: "text".into(),
                    text: "hello ".into(),
                    extra: Default::default(),
                },
                ContentPiece {
                    piece_type: "text".into(),
                    text: "world".into(),
                    extra: Default::default(),
                },
            ],
            sender: "human".into(),
            created_at: "t".into(),
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
            out[0].created_at, "t1",
            "kept human message must be the later resend"
        );
    }

    /// Case 2 — a stray reply to an early attempt: human, assistant (to the
    /// failed attempt), human (identical resend), assistant (real reply) —
    /// the stray assistant reply is dropped along with the first human attempt.
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
        assert_eq!(out[0].created_at, "t2");
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
        assert_eq!(out[2].created_at, "t3");
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
            out[0].created_at, "t5",
            "only the last of the 5 resends survives"
        );
    }

    /// Case 6 — two independent duplicate runs in the same conversation,
    /// each resolved on its own.
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

    /// A run broken by a genuinely different human message ends the scan —
    /// the second, different human message starts its own fresh run.
    #[test]
    fn different_human_message_breaks_the_run() {
        let msgs = vec![human("a", "t0"), human("a", "t1"), human("different", "t2")];
        let out = dedup_chat_messages(&msgs);
        assert_eq!(texts(&out), vec!["a", "different"]);
        assert_eq!(out[0].created_at, "t1");
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
            /// human subsequence) have identical text — that's the whole
            /// point of the run-collapsing rule.
            #[test]
            fn no_adjacent_identical_human_messages_survive(msgs in arb_message_seq()) {
                let out = dedup_chat_messages(&msgs);
                let human_texts: Vec<String> = out
                    .iter()
                    .filter(|m| m.sender == "human")
                    .map(extract_text)
                    .collect();
                for pair in human_texts.windows(2) {
                    prop_assert_ne!(&pair[0], &pair[1]);
                }
            }

            /// Every surviving message was present, unchanged, in the input
            /// — dedup only removes, it never rewrites a message.
            #[test]
            fn every_surviving_message_is_identical_to_some_input_message(msgs in arb_message_seq()) {
                let out = dedup_chat_messages(&msgs);
                for m in &out {
                    prop_assert!(msgs.contains(m));
                }
            }
        }
    }

    #[test]
    fn dedup_conversations_runs_on_every_conversation() {
        let mut convs = vec![
            crate::model::Conversation {
                uuid: "c0".into(),
                name: "conv 0".into(),
                chat_messages: vec![human("a", "t0"), human("a", "t1")],
                extra: Default::default(),
            },
            crate::model::Conversation {
                uuid: "c1".into(),
                name: "conv 1".into(),
                chat_messages: vec![human("b", "t0"), assistant("reply", "t1")],
                extra: Default::default(),
            },
        ];
        dedup_conversations(&mut convs);
        assert_eq!(convs[0].chat_messages.len(), 1);
        assert_eq!(convs[1].chat_messages.len(), 2);
    }
}
