//! Processing an upload into rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4–§4e, §7, §7b,
//! §8b), through `process_upload` and the routes that read what it wrote.

#[path = "support/exports.rs"]
mod exports;
#[path = "support/local_app.rs"]
mod local_app;

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use exports::{at, claude, conv, conversation, export, msg, you};
use serde_json::{json, Value};
use timeline_api::processing::{process_upload, ProcessingError, LARGEST_ROW};
use timeline_api::s3_trigger::ProcessingStores;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, UploadOutcome};
use timeline_core::FormatError;

fn alice() -> UserId {
    UserId("alice".to_string())
}

/// Puts `bytes` in place as a new upload of alice's with its facts
/// recorded, and processes it.
async fn process(
    stores: &ProcessingStores,
    bytes: Vec<u8>,
) -> (UploadId, Result<(), ProcessingError>) {
    let upload = UploadId(uuid::Uuid::new_v4());
    stores
        .object_store
        .put(&raw_object_key(&alice(), upload), bytes)
        .await
        .unwrap();
    local_app::record_upload_facts(stores, &alice(), upload).await;
    let result = process_upload(stores, &alice(), upload).await;
    (upload, result)
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// The page sends its slimmed file gzip-compressed (§7b); processing
/// decompresses it, and still reads a plain file.
#[tokio::test]
async fn a_gzip_upload_is_read_like_a_plain_one() {
    let stores = local_app::memory_stores();
    let raw = export(vec![conversation(
        1,
        "One",
        &[you(0, "hi"), claude(1, "hello")],
    )]);
    let (_, result) = process(&stores, gzip(raw.as_bytes())).await;
    result.unwrap();
    let records = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message_count, 2);
}

#[tokio::test]
async fn a_gzip_upload_that_doesnt_decompress_is_refused_and_recorded() {
    let stores = local_app::memory_stores();
    let mut broken = gzip(b"[]");
    broken.truncate(8);
    let (upload, result) = process(&stores, broken).await;
    let err = result.unwrap_err();
    assert!(matches!(err, ProcessingError::Decompress(_)), "{err:?}");
    assert!(err.is_unusable_file());
    assert!(std::error::Error::source(&err).is_some());
    let outcome = stores
        .upload_outcome_store
        .get_outcome(&alice(), upload)
        .await
        .unwrap();
    let Some(UploadOutcome::Failed { reason }) = outcome else {
        panic!("{outcome:?}")
    };
    assert!(reason.contains("could not be decompressed"), "{reason}");
}

/// The two accepted shapes, read as a stream: anything else is the
/// unrecognised shape, broken JSON is invalid JSON, a conversation that
/// doesn't match the schema is an invalid conversation, and text that
/// isn't UTF-8 says so.
#[tokio::test]
async fn each_kind_of_unreadable_file_is_named() {
    for (bytes, expected) in [
        (b"true".to_vec(), "unrecognised"),
        (b"\"a string\"".to_vec(), "unrecognised"),
        (b"{}".to_vec(), "unrecognised"),
        (b"{\"conversations\": 5}".to_vec(), "unrecognised"),
        (b"{\"conversations\": {}}".to_vec(), "unrecognised"),
        (b"[{\"uuid\": ".to_vec(), "invalid json"),
        // Read as a stream, the first element fails before the broken end.
        (b"[1, 2".to_vec(), "invalid conversation"),
        (b"[] trailing".to_vec(), "invalid json"),
        (
            b"[{\"name\": \"no uuid\"}]".to_vec(),
            "invalid conversation",
        ),
        (vec![b'[', 0xff, b']'], "not utf-8"),
    ] {
        let stores = local_app::memory_stores();
        let (_, result) = process(&stores, bytes.clone()).await;
        let err = result.unwrap_err();
        let kind = match &err {
            ProcessingError::Format(FormatError::UnrecognizedShape) => "unrecognised",
            ProcessingError::Format(FormatError::InvalidJson(_)) => "invalid json",
            ProcessingError::Format(FormatError::InvalidConversation(_)) => "invalid conversation",
            ProcessingError::RawObjectNotUtf8(_) => "not utf-8",
            other => panic!("{other:?}"),
        };
        assert_eq!(kind, expected, "{}", String::from_utf8_lossy(&bytes));
        assert!(err.is_unusable_file());
    }
}

/// A wrapped file (an annotated download) is read with its other keys
/// ignored, in any order, and its conversations kept as they are.
#[tokio::test]
async fn a_wrapped_file_is_read_whatever_its_other_keys() {
    let stores = local_app::memory_stores();
    let wrapped = json!({
        "first": {"ignored": [1, 2]},
        "conversations": [conversation(1, "One", &[you(0, "again"), you(1, "again")])],
        "claude_timeline_format_version": "3",
    });
    let (_, result) = process(&stores, wrapped.to_string().into_bytes()).await;
    result.unwrap();
    let records = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(
        records[0].message_count, 2,
        "an annotated download isn't cleaned again"
    );
}

/// Messages with no time are kept (§4e): counted, the conversation placed
/// by its record's span, and the count recorded as `untimed`.
#[tokio::test]
async fn messages_of_unknown_time_are_kept_and_counted() {
    let (router, _, _) = local_app::app();
    let mut c = conversation(1, "Undated", &[you(0, "timed")]);
    c["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 9), "sender": "assistant", "created_at": null,
        "content": [{"type": "text", "text": "when?"}],
    }));
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let records = local_app::conversations(&router, &token).await;
    assert_eq!(records[0]["message_count"], 2);
    assert_eq!(records[0]["untimed"], 1);
    let sessions = local_app::sessions(&router, &token).await;
    assert_eq!(sessions[0]["placement"], "span");
    assert_eq!(sessions[0]["message_count"], 2);
    // The download leaves its time out, so uploading it again keeps it unknown.
    let file = local_app::exported(&router, &token).await;
    let untimed = file["conversations"][0]["chat_messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["uuid"] == msg(1, 9))
        .unwrap()
        .clone();
    assert!(untimed.get("created_at").is_none(), "{untimed}");
}

/// A message too large for one stored row is refused, naming it (§8b).
#[tokio::test]
async fn a_message_too_large_to_store_is_refused_naming_it() {
    let stores = local_app::memory_stores();
    let huge = "x".repeat(LARGEST_ROW + 10);
    let raw = export(vec![conversation(1, "Huge", &[you(0, &huge)])]);
    let (upload, result) = process(&stores, raw.into_bytes()).await;
    let err = result.unwrap_err();
    assert!(
        matches!(err, ProcessingError::MessageTooLarge { .. }),
        "{err:?}"
    );
    assert!(err.is_unusable_file());
    assert!(std::error::Error::source(&err).is_none());
    let text = err.to_string();
    assert!(
        text.contains(&msg(1, 1)) && text.contains(&conv(1)),
        "{text}"
    );
    assert!(stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap()
        .is_empty());
    assert!(matches!(
        stores
            .upload_outcome_store
            .get_outcome(&alice(), upload)
            .await
            .unwrap(),
        Some(UploadOutcome::Failed { .. })
    ));
}

/// The upload is deleted once processed (the user, Q2); an upload already
/// processed is skipped, so a repeated S3 event doesn't fail on the missing
/// file and overwrite "ready" with "failed" (§10b).
#[tokio::test]
async fn the_upload_is_deleted_once_processed_and_a_repeat_is_skipped() {
    let stores = local_app::memory_stores();
    let raw = export(vec![conversation(1, "One", &[you(0, "hi")])]);
    let (upload, result) = process(&stores, raw.clone().into_bytes()).await;
    result.unwrap();
    let key = raw_object_key(&alice(), upload);
    assert!(matches!(
        stores.object_store.get(&key).await,
        Err(ObjectStoreError::NotFound)
    ));
    let (version_before, _) = (
        stores
            .user_records
            .get(&alice())
            .await
            .unwrap()
            .data_version,
        (),
    );
    process_upload(&stores, &alice(), upload).await.unwrap();
    assert!(matches!(
        stores
            .upload_outcome_store
            .get_outcome(&alice(), upload)
            .await
            .unwrap(),
        Some(UploadOutcome::Ready { .. })
    ));
    assert_eq!(
        stores
            .user_records
            .get(&alice())
            .await
            .unwrap()
            .data_version,
        version_before,
        "a skipped upload changes nothing"
    );
    // Had the file been left (a failed delete), the repeat deletes it.
    stores
        .object_store
        .put(&key, raw.into_bytes())
        .await
        .unwrap();
    process_upload(&stores, &alice(), upload).await.unwrap();
    assert!(matches!(
        stores.object_store.get(&key).await,
        Err(ObjectStoreError::NotFound)
    ));
}

/// Totals are counted afresh after every upload and the data version
/// raised (§5c, §8b).
#[tokio::test]
async fn totals_are_counted_afresh_and_the_version_raised() {
    let stores = local_app::memory_stores();
    process(
        &stores,
        export(vec![conversation(
            1,
            "One",
            &[you(0, "a"), claude(1, "b"), you(60, "c")],
        )])
        .into_bytes(),
    )
    .await
    .1
    .unwrap();
    let first = stores.user_records.get(&alice()).await.unwrap();
    assert_eq!(first.data_version, 1);
    assert_eq!(
        (
            first.totals.conversations,
            first.totals.sessions,
            first.totals.your_messages,
            first.totals.messages
        ),
        (1, 2, 2, 3)
    );
    process(
        &stores,
        export(vec![conversation(2, "Two", &[you(0, "d")])]).into_bytes(),
    )
    .await
    .1
    .unwrap();
    let second = stores.user_records.get(&alice()).await.unwrap();
    assert_eq!(second.data_version, 2);
    assert_eq!(
        (second.totals.conversations, second.totals.messages),
        (2, 4)
    );
}

/// Plan §12.2: a conversation with no messages is dropped: no record, and
/// not counted in the totals.
#[tokio::test]
async fn a_conversation_with_no_messages_is_not_stored_or_counted() {
    let stores = local_app::memory_stores();
    process(
        &stores,
        export(vec![
            conversation(1, "One", &[you(0, "a")]),
            conversation(2, "", &[]),
        ])
        .into_bytes(),
    )
    .await
    .1
    .unwrap();
    let records = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(
        records
            .iter()
            .map(|r| r.name.0.as_str())
            .collect::<Vec<_>>(),
        vec!["One"]
    );
    let user = stores.user_records.get(&alice()).await.unwrap();
    assert_eq!(user.totals.conversations, 1);
}

/// Files Claude wrote and presented are stored under their message and
/// number, and listed and handed out by the routes (§4).
#[tokio::test]
async fn files_are_stored_listed_and_handed_out() {
    let (router, _, _) = local_app::app();
    let mut c = conversation(1, "Files", &[you(0, "write a plan"), claude(1, "Here:")]);
    c["chat_messages"][1]["content"] = json!([
        {"type": "tool_use", "name": "create_file", "input": {"path": "/out/plan.md", "file_text": "# Plan"}},
        {"type": "text", "text": "Here it is:"},
        {"type": "tool_use", "name": "present_files", "input": {"filepaths": ["/out/plan.md", "/out/deck.pptx"]}},
    ]);
    c["chat_messages"][0]["attachments"] =
        json!([{"file_name": "brief.txt", "extracted_content": "the brief"}]);
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let files = local_app::all_of(
        &router,
        &token,
        &format!("/conversations/{}/files", conv(1)),
        "files",
    )
    .await;
    let listed: Vec<(Value, Value, Value)> = files
        .iter()
        .map(|f| {
            (
                f["name"].clone(),
                f["contents"].clone(),
                f["sender"].clone(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        vec![
            (json!("brief.txt"), json!("stored"), json!("human")),
            (json!("plan.md"), json!("stored"), json!("assistant")),
            (
                json!("deck.pptx"),
                json!("not_in_export"),
                json!("assistant")
            ),
        ]
    );
    let plan = &files[1];
    assert_eq!(plan["kind"], json!({"kind": "markdown"}));
    let (status, address) = local_app::get(
        &router,
        &token,
        &format!(
            "/files/{}/{}/{}",
            conv(1),
            plan["message_id"].as_str().unwrap(),
            plan["number"]
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{address}");
    assert_eq!(address["name"], "plan.md");
    let (status, text) = local_app::send(
        &router,
        axum::http::Request::builder()
            .uri(address["url"].as_str().unwrap())
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text, json!("# Plan"));
    // A file whose contents aren't stored, another number, another user's,
    // and a message that isn't there: 404 alike.
    for uri in [
        format!("/files/{}/{}/1", conv(1), msg(1, 2)),
        format!("/files/{}/{}/7", conv(1), msg(1, 2)),
        format!("/files/{}/{}/0", conv(1), msg(1, 99)),
    ] {
        let (status, _) = local_app::get(&router, &token, &uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    let bob = local_app::dev_login(&router, "bob").await;
    let (status, _) = local_app::get(
        &router,
        &bob,
        &format!("/files/{}/{}/0", conv(1), msg(1, 2)),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // Review carries the file mark in its place in the reply.
    let rows = local_app::review_rows(&router, &token, "replies=true").await;
    let pieces = &rows[0]["reply"]["pieces"];
    assert_eq!(pieces[0]["type"], "text");
    assert_eq!(pieces[1]["type"], "file");
    assert_eq!(pieces[1]["file"]["name"], "plan.md");
    assert_eq!(rows[0]["attachments"][0]["name"], "brief.txt");
}

/// A replaced branch of 100 words or more is kept as a conversation of its
/// own, linked both ways, and the note says so (§4d).
#[tokio::test]
async fn an_important_branch_becomes_a_linked_conversation_of_its_own() {
    let (router, _, _) = local_app::app();
    let long = vec!["dictated"; 130].join(" ");
    let mut c = conversation(
        1,
        "Banking",
        &[you(0, "start"), claude(1, "ok"), you(10, "asked again")],
    );
    c["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 9), "parent_message_uuid": msg(1, 2), "sender": "human",
        "created_at": at(5), "content": [{"type": "text", "text": long}],
    }));
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let records = local_app::conversations(&router, &token).await;
    assert_eq!(records.len(), 2);
    let main = records
        .iter()
        .find(|r| r["conversation_id"] == conv(1))
        .unwrap();
    let branch = records
        .iter()
        .find(|r| r["conversation_id"] != conv(1))
        .unwrap();
    assert_eq!(main["branches"], json!([branch["conversation_id"]]));
    assert_eq!(branch["branch_of"], conv(1));
    assert_eq!(
        branch["name"],
        "Banking: earlier branch from 2026-03-02 09:05 UTC"
    );
    assert_eq!(branch["message_count"], 1);
    assert_eq!(
        main["message_count"], 3,
        "the branch's message isn't counted twice"
    );
    let rows = local_app::review_rows(&router, &token, &format!("conversation={}", conv(1))).await;
    let note = rows.iter().find(|r| r["kind"] == "note").unwrap();
    assert_eq!(note["kept_as"], branch["conversation_id"]);
    assert_eq!(note["words_not_repeated"], 130);
    let outcome_ids = local_app::all_of(&router, &token, "/uploads", "uploads").await[0]
        ["conversation_count"]
        .clone();
    assert_eq!(outcome_ids, 2, "the branch came in this file too");
}

/// C12, through processing: a later file whose newest messages continue a
/// pruned branch makes it the path; the stored rows after the branch point
/// are replaced by a note, and the branch's messages added.
#[tokio::test]
async fn a_revived_branch_becomes_the_path_on_a_later_upload() {
    let (router, _, _) = local_app::app();
    let token = local_app::dev_login(&router, "alice").await;
    let first = conversation(
        1,
        "Revived",
        &[
            you(0, "start"),
            claude(1, "ok"),
            you(10, "path one"),
            claude(11, "reply one"),
        ],
    );
    local_app::upload(&router, &token, export(vec![first.clone()])).await;
    let mut later = first;
    for (n, parent, minute, text) in [
        (7, 2, 5, "the branch"),
        (8, 7, 6, "branch reply"),
        (9, 8, 90, "newest"),
    ] {
        later["chat_messages"].as_array_mut().unwrap().push(json!({
            "uuid": msg(1, n), "parent_message_uuid": msg(1, parent), "sender": if n == 8 { "assistant" } else { "human" },
            "created_at": at(minute), "content": [{"type": "text", "text": text}],
        }));
    }
    local_app::upload(&router, &token, export(vec![later])).await;
    let rows = local_app::review_rows(&router, &token, "").await;
    let shown: Vec<String> = rows
        .iter()
        .map(|r| r["message_id"].as_str().unwrap_or("note").to_string())
        .collect();
    // Your messages in time order: the start, the note where the old path
    // began (at 10), the branch (5) and the newest (90).
    assert_eq!(
        shown,
        vec![msg(1, 1), msg(1, 7), "note".to_string(), msg(1, 9)],
        "{rows:?}"
    );
    assert!(
        shown.contains(&msg(1, 7)) && shown.contains(&msg(1, 9)),
        "{shown:?}"
    );
    assert!(
        !shown.contains(&msg(1, 3)),
        "the replaced path is gone: {shown:?}"
    );
    let note = rows.iter().find(|r| r["kind"] == "note").unwrap();
    assert_eq!(note["replaced_by"], msg(1, 7));
    assert_eq!(note["messages"], 2);
    let record = &local_app::conversations(&router, &token).await[0];
    assert_eq!(
        record["message_count"], 5,
        "start, ok, branch, its reply, newest"
    );
}

/// C1: two files of a batch processed at once can both read a
/// conversation's record; the second write finds a newer version and is
/// refused, and processing reads the record again and redoes the
/// conversation as a merge, so neither file's messages are lost.
#[tokio::test]
async fn a_lost_race_is_redone_as_a_merge() {
    struct RacingRecords {
        inner: Arc<dyn ConversationSummaryStore>,
        rival: ConversationSummary,
        raced: AtomicUsize,
    }
    #[async_trait]
    impl ConversationSummaryStore for RacingRecords {
        async fn list_for_user(
            &self,
            user: &UserId,
        ) -> Result<Vec<ConversationSummary>, StoreError> {
            self.inner.list_for_user(user).await
        }
        async fn list_page(
            &self,
            user: &UserId,
            after: Option<ConversationId>,
            max: usize,
        ) -> Result<Vec<ConversationSummary>, StoreError> {
            self.inner.list_page(user, after, max).await
        }
        async fn get(
            &self,
            user: &UserId,
            id: ConversationId,
        ) -> Result<Option<ConversationSummary>, StoreError> {
            self.inner.get(user, id).await
        }
        async fn put(
            &self,
            user: &UserId,
            summary: ConversationSummary,
        ) -> Result<ConversationSummary, StoreError> {
            // The rival file stores the conversation just before this write.
            if self.raced.fetch_add(1, Ordering::SeqCst) == 0 {
                self.inner.put(user, self.rival.clone()).await?;
            }
            self.inner.put(user, summary).await
        }
    }

    // The rival's copy: the conversation's first message alone, stored
    // with its rows by the rival's own processing.
    let rival_stores = local_app::memory_stores();
    let short = export(vec![conversation(1, "Raced", &[you(0, "first")])]);
    process(&rival_stores, short.into_bytes()).await.1.unwrap();
    let rival = rival_stores
        .conversation_summary_store
        .get(&alice(), ConversationId(conv(1).parse().unwrap()))
        .await
        .unwrap()
        .unwrap();

    let mut stores = rival_stores.clone();
    let blank_records: Arc<dyn ConversationSummaryStore> =
        Arc::new(timeline_storage::memory::conversations::InMemoryConversationSummaryStore::new());
    stores.conversation_summary_store = Arc::new(RacingRecords {
        inner: blank_records.clone(),
        rival: ConversationSummary {
            version: 0,
            ..rival
        },
        raced: AtomicUsize::new(0),
    });
    let long = export(vec![conversation(
        1,
        "Raced",
        &[you(0, "first"), claude(1, "reply"), you(30, "later")],
    )]);
    process(&stores, long.into_bytes()).await.1.unwrap();
    let record = blank_records
        .get(&alice(), ConversationId(conv(1).parse().unwrap()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.version, 2,
        "the rival's write, then the redone merge"
    );
    assert_eq!(record.message_count, 3, "nothing lost: {record:?}");
}

/// While processing runs, how far it has got is written every second
/// (§8b); the page reads it through `GET /uploads/{id}`.
#[tokio::test]
async fn progress_is_written_every_second_while_processing_runs() {
    struct SlowRows(ProcessingStores);
    #[async_trait]
    impl timeline_core::ports::messages::MessageRowWriter for SlowRows {
        async fn put_entries(
            &self,
            user: &UserId,
            entries: &[timeline_core::stored_message::Entry],
        ) -> Result<(), StoreError> {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
            self.0.message_writer.put_entries(user, entries).await
        }
        async fn delete_entries(
            &self,
            user: &UserId,
            keys: &[timeline_core::stored_message::EntryKey],
        ) -> Result<(), StoreError> {
            self.0.message_writer.delete_entries(user, keys).await
        }
    }
    let base = local_app::memory_stores();
    let mut stores = base.clone();
    stores.message_writer = Arc::new(SlowRows(base));
    let raw = export(vec![
        conversation(1, "One", &[you(0, "a")]),
        conversation(2, "Two", &[you(0, "b")]),
    ]);
    let (upload, result) = process(&stores, raw.clone().into_bytes()).await;
    result.unwrap();
    let progress = stores
        .upload_outcome_store
        .get_progress(&alice(), upload)
        .await
        .unwrap()
        .unwrap();
    let p = progress.processing.expect("written at least once");
    assert_eq!(p.bytes_total, raw.len() as u64);
    assert_eq!(p.bytes_read, p.bytes_total);
    assert_eq!(p.conversations_total, 2);
    assert!(p.conversations_written >= 1);
}

/// Editing the start and end of a conversation placed by them (§4e) moves
/// its session and raises the data version; one cut by its messages' times
/// keeps its sessions.
#[tokio::test]
async fn editing_a_placed_conversations_span_moves_its_session() {
    let (router, _, _) = local_app::app();
    let mut c = conversation(1, "Undated", &[you(0, "timed")]);
    c["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 9), "sender": "human", "content": [{"type": "text", "text": "when?"}],
    }));
    let timed = conversation(2, "Timed", &[you(0, "x")]);
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c, timed])).await;
    let (_, before) = local_app::get(&router, &token, "/sessions").await;
    let span = json!({"start": "2026-05-01T10:00:00Z", "end": "2026-05-01T11:00:00Z"});
    for id in [conv(1), conv(2)] {
        let (status, _) = local_app::send(
            &router,
            local_app::request(
                "PUT",
                &format!("/conversations/{id}/metadata"),
                &token,
                Some(json!({"span": span})),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (_, after) = local_app::get(&router, &token, "/sessions").await;
    let placed = after["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["conversation_id"] == conv(1))
        .unwrap();
    assert_eq!(
        (placed["start"].clone(), placed["end"].clone()),
        (json!("2026-05-01T10:00:00Z"), json!("2026-05-01T11:00:00Z"))
    );
    let cut = after["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["conversation_id"] == conv(2))
        .unwrap();
    assert_eq!(
        cut["start"],
        at(0).replace("+00:00", "Z"),
        "a timed conversation's sessions stay"
    );
    assert_eq!(
        after["data_version"].as_u64(),
        before["data_version"].as_u64().map(|v| v + 1)
    );
}

/// The annotated download carries citations and file names, and uploading
/// it again brings your reviews back (it replaces `export-format.test.js`
/// L16, "a wrapped export is read as already processed").
#[tokio::test]
async fn the_download_uploaded_again_brings_your_reviews_back() {
    let (router, _, _) = local_app::app();
    let mut c = conversation(
        1,
        "Round trip",
        &[you(0, "WRONG"), claude(1, "Sourced claim.")],
    );
    c["chat_messages"][1]["content"][0]["citations"] =
        json!([{"start_index": 0, "end_index": 7, "details": {"url": "https://example.com/s"}}]);
    c["chat_messages"][0]["_claude_timeline_user"] = json!({"angry": true});
    c["chat_messages"][0]["files"] = json!([{"file_name": "scan.pdf"}]);
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    local_app::scan(&router, &token).await;
    let (text, _) = local_app::export(&router, &token).await;
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["claude_timeline_format_version"], "3");
    let messages = file["conversations"][0]["chat_messages"]
        .as_array()
        .unwrap();
    assert_eq!(
        messages[0]["_claude_timeline_user"],
        json!({"caps": null, "critical": null, "angry": true})
    );
    assert_eq!(messages[0]["_claude_timeline_auto"]["caps"], true);
    assert_eq!(messages[0]["files"], json!([{"file_name": "scan.pdf"}]));
    assert!(
        messages[0].get("parent_message_uuid").is_none(),
        "the conversation's start has none"
    );
    assert_eq!(messages[1]["parent_message_uuid"], msg(1, 1));
    assert_eq!(
        messages[1]["content"][0]["citations"][0]["details"]["url"],
        "https://example.com/s"
    );
    assert!(messages[1].get("_claude_timeline_auto").is_none());

    let (other, _, _) = local_app::app();
    let again = local_app::signed_in_with(&other, "alice", &text).await;
    let rows = local_app::review_rows(&other, &again, "").await;
    assert_eq!(rows[0]["flags"]["user"]["angry"], true);
    assert_eq!(
        rows[0]["flags"]["auto"],
        Value::Null,
        "automatic flags are never taken from a file"
    );
}

/// A download larger than one part comes in several, each well under
/// Lambda's limit on an answer, and they join into one file.
#[tokio::test]
async fn a_large_download_comes_in_parts_of_at_most_4_mb() {
    let (router, _, _) = local_app::app();
    // Distinct texts: identical ones in a row would be removed as resends.
    let texts: Vec<String> = (0..20)
        .map(|i| format!("{i} {}", "y".repeat(300_000)))
        .collect();
    let messages: Vec<exports::M> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| you(i as i64 * 30, t))
        .collect();
    let token = local_app::signed_in_with(
        &router,
        "alice",
        &export(vec![conversation(1, "Big", &messages)]),
    )
    .await;
    let parts = local_app::all_parts(&router, &token, "/export").await;
    assert!(parts.len() >= 2, "{} parts", parts.len());
    for part in &parts {
        assert!(part["part"].as_str().unwrap().len() < 4 * 1024 * 1024 + 400_000);
    }
    let joined: String = parts.iter().map(|p| p["part"].as_str().unwrap()).collect();
    let file: Value = serde_json::from_str(&joined).unwrap();
    assert_eq!(
        file["conversations"][0]["chat_messages"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
}
