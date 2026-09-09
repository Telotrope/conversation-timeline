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
