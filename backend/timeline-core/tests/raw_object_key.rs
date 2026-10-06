//! `raw_object_key` and its inverse, `parse_raw_object_key`. The processing
//! Lambda traces each S3 event back to its upload with the parser, so a key
//! the writer produces must always parse back, and nothing else may.

use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{parse_raw_object_key, raw_object_key};

const UPLOAD: &str = "6f1c1b6e-2f4e-4a8e-9a57-0b6f1d6a9c11";

fn upload_id() -> UploadId {
    UploadId(UPLOAD.parse().unwrap())
}

#[test]
fn every_written_key_parses_back_to_its_user_and_upload() {
    let user = UserId("a1b2c3d4-0000-4000-8000-000000000000".to_string());
    let key = raw_object_key(&user, upload_id());
    assert_eq!(parse_raw_object_key(&key), Some((user, upload_id())));
}

#[test]
fn keys_the_writer_cannot_produce_are_not_parsed() {
    for key in [
        format!("export/alice/{UPLOAD}.json"),
        format!("raw//{UPLOAD}.json"),
        format!("raw/alice/{UPLOAD}"),
        "raw/alice/not-a-uuid.json".to_string(),
        "raw/alice".to_string(),
        String::new(),
    ] {
        assert_eq!(parse_raw_object_key(&key), None, "{key:?}");
    }
}

/// Replaces `added_messages_are_kept_under_their_user_conversation_and_upload`
/// (plan 2026-10-06-load-only-what-the-page-shows.md §10b): the additions
/// objects are gone; files kept from a conversation are stored under their
/// user, conversation, message and number, and no file name becomes part
/// of the key.
#[test]
fn a_stored_file_is_kept_under_its_user_conversation_message_and_number() {
    let user = UserId("alice".to_string());
    let conversation = timeline_core::ConversationId(uuid::Uuid::from_u128(5));
    let message = timeline_core::MessageId(uuid::Uuid::from_u128(6));
    let key = timeline_core::ports::uploads::file_object_key(&user, conversation, message, 2);
    assert_eq!(
        key,
        "files/alice/00000000-0000-0000-0000-000000000005/00000000-0000-0000-0000-000000000006/2"
    );
    // Never mistaken for a raw upload, whose arrival starts processing.
    assert_eq!(parse_raw_object_key(&key), None);
}
