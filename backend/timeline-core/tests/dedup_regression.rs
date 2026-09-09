//! Regression test against a real Anthropic export excerpt — not a
//! synthetic fixture. See the migration plan's C1/C8/C9 for provenance: this
//! is a trimmed, format-preserving excerpt of a real `conversations.json`
//! export, selected specifically because 3 of its 6 conversations contain a
//! genuine resend-after-empty-assistant-reply duplicate (the exact
//! real-world case `dedup_chat_messages` exists for).

use timeline_core::dedup_conversations;
use timeline_core::model::Conversation;

const FIXTURE: &str = include_str!("fixtures/sample_conversations.json");

#[test]
fn real_sample_export_dedups_to_the_measured_count() {
    let mut conversations: Vec<Conversation> = serde_json::from_str(FIXTURE)
        .expect("fixture is valid JSON matching the Conversation schema");
    assert_eq!(conversations.len(), 6);

    let raw_total: usize = conversations.iter().map(|c| c.chat_messages.len()).sum();
    assert_eq!(
        raw_total, 36,
        "raw message count in the fixture should not silently drift"
    );

    dedup_conversations(&mut conversations);

    let deduped_total: usize = conversations.iter().map(|c| c.chat_messages.len()).sum();
    assert_eq!(
        deduped_total, 30,
        "expected 6 duplicate messages dropped, matching the measured full-export ratio"
    );

    let conversations_with_drops = conversations
        .iter()
        .zip(&[0usize, 2, 2, 18, 7, 7]) // original per-conversation raw counts, in fixture order
        .filter(|(c, &raw_len)| c.chat_messages.len() < raw_len)
        .count();
    assert_eq!(
        conversations_with_drops, 3,
        "exactly 3 of the 6 conversations should contain a duplicate run"
    );
}

#[test]
fn dedup_is_idempotent_on_already_deduplicated_real_data() {
    let mut conversations: Vec<Conversation> = serde_json::from_str(FIXTURE)
        .expect("fixture is valid JSON matching the Conversation schema");
    dedup_conversations(&mut conversations);
    let once: Vec<usize> = conversations
        .iter()
        .map(|c| c.chat_messages.len())
        .collect();

    dedup_conversations(&mut conversations);
    let twice: Vec<usize> = conversations
        .iter()
        .map(|c| c.chat_messages.len())
        .collect();

    assert_eq!(
        once, twice,
        "running dedup on already-deduped data must be a no-op"
    );
}
