//! The less common paths through processing (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §4d, §7, §8b):
//! a revived branch moved with its files, a later file's new branch, a
//! retried attempt meeting what an earlier one wrote, a record store that
//! keeps losing races, failures in each step naming it, and progress
//! written while the file is parsed.

#[path = "support/exports.rs"]
mod exports;
#[path = "support/local_app.rs"]
mod local_app;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use exports::{at, claude, conv, conversation, export, msg, you};
use serde_json::{json, Value};
use timeline_api::processing::{process_upload, process_upload_reporting, ProcessingError};
use timeline_api::request_record::recording;
use timeline_api::s3_trigger::ProcessingStores;
use timeline_core::model::ConversationId;
use timeline_core::ports::conversations::{ConversationSummary, ConversationSummaryStore};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::uploads::{file_object_key, raw_object_key, UploadOutcome};
use timeline_core::stored_session::{SessionKey, StoredSession};

fn alice() -> UserId {
    UserId("alice".to_string())
}

async fn put_upload(stores: &ProcessingStores, raw: String) -> UploadId {
    let upload = UploadId(uuid::Uuid::new_v4());
    stores
        .object_store
        .put(&raw_object_key(&alice(), upload), raw.into_bytes())
        .await
        .unwrap();
    local_app::record_upload_facts(stores, &alice(), upload).await;
    upload
}

async fn process(stores: &ProcessingStores, raw: String) -> Result<(), ProcessingError> {
    let upload = put_upload(stores, raw).await;
    process_upload(stores, &alice(), upload).await
}

fn human(n: u32, parent: u32, minute: i64, text: &str) -> Value {
    json!({"uuid": msg(1, n), "parent_message_uuid": msg(1, parent), "sender": "human",
           "created_at": at(minute), "content": [{"type": "text", "text": text}]})
}

/// C12 with an important replaced path: the stored path after the branch
/// point is moved into a conversation of its own, its stored file moved
/// with it, and the new path, now unbroken, is one session where there were
/// two.
#[tokio::test]
async fn a_revived_branch_moves_an_important_replaced_path_with_its_files() {
    let stores = local_app::memory_stores();
    let long = vec!["dictated"; 120].join(" ");
    let mut first = conversation(1, "Moved", &[you(0, "start"), claude(1, "ok")]);
    let mut path = human(3, 2, 40, &long);
    path["attachments"] = json!([{"file_name": "notes.txt", "extracted_content": "my notes"}]);
    first["chat_messages"].as_array_mut().unwrap().push(path);
    process(&stores, export(vec![first.clone()])).await.unwrap();
    let old_sessions = stores
        .session_store
        .sessions_of(&alice(), ConversationId(conv(1).parse().unwrap()))
        .await
        .unwrap();
    assert_eq!(old_sessions.len(), 2, "0-1 and 40");

    let mut later = first;
    for (n, parent, minute, text) in [
        (7, 2, 10, "a"),
        (8, 7, 20, "b"),
        (9, 8, 30, "c"),
        (10, 9, 50, "newest"),
    ] {
        later["chat_messages"]
            .as_array_mut()
            .unwrap()
            .push(human(n, parent, minute, text));
    }
    let (result, record) = recording(process(&stores, export(vec![later]))).await;
    result.unwrap();
    assert_eq!(
        record.facts["conversations_gained_messages"], 1,
        "{:?}",
        record.facts
    );

    let main_id = ConversationId(conv(1).parse().unwrap());
    let main = stores
        .conversation_summary_store
        .get(&alice(), main_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(main.branches.len(), 1);
    let moved_id = main.branches[0];
    let moved = stores
        .conversation_summary_store
        .get(&alice(), moved_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(moved.branch_of, Some(main_id));
    assert_eq!(moved.message_count, 1);
    assert_eq!(
        moved.name.0,
        "Moved: earlier branch from 2026-03-02 09:40 UTC"
    );
    // The attachment's text moved with its message.
    let message = timeline_core::model::MessageId(msg(1, 3).parse().unwrap());
    assert_eq!(
        stores
            .object_store
            .get(&file_object_key(&alice(), moved_id, message, 0))
            .await
            .unwrap(),
        b"my notes"
    );
    assert!(matches!(
        stores
            .object_store
            .get(&file_object_key(&alice(), main_id, message, 0))
            .await,
        Err(ObjectStoreError::NotFound)
    ));
    // start 0, ok 1, a 10, b 20, c 30, the note at 40, newest 50: no pause of
    // 15 minutes, so one session where there were two.
    let sessions = stores
        .session_store
        .sessions_of(&alice(), main_id)
        .await
        .unwrap();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    assert_eq!(main.message_count, 6);
}

/// A later file whose new part holds an important replaced branch keeps it
/// as a conversation of its own, as the first upload would have.
#[tokio::test]
async fn a_later_files_new_important_branch_is_kept_as_its_own_conversation() {
    let stores = local_app::memory_stores();
    let first = conversation(1, "Later", &[you(0, "start"), claude(1, "ok")]);
    process(&stores, export(vec![first.clone()])).await.unwrap();
    let long = vec!["unanswered"; 110].join(" ");
    let mut later = first;
    for (n, parent, minute, text) in [(3, 2, 60, long.as_str()), (4, 2, 61, "asked again")] {
        later["chat_messages"]
            .as_array_mut()
            .unwrap()
            .push(human(n, parent, minute, text));
    }
    process(&stores, export(vec![later])).await.unwrap();
    let records = stores
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    let main = records
        .iter()
        .find(|r| r.conversation_id.to_string() == conv(1))
        .unwrap();
    let branch = records
        .iter()
        .find(|r| r.conversation_id.to_string() != conv(1))
        .unwrap();
    assert_eq!(main.branches, vec![branch.conversation_id]);
    assert_eq!(branch.branch_of, Some(main.conversation_id));
    assert_eq!(main.message_count, 3);
}

/// A record store that fails its `fail_on`th write, then works.
struct RecordsFailingOnce {
    inner: Arc<dyn ConversationSummaryStore>,
    fail_on: usize,
    writes: AtomicUsize,
    error: fn() -> StoreError,
}

#[async_trait]
impl ConversationSummaryStore for RecordsFailingOnce {
    async fn list_for_user(&self, user: &UserId) -> Result<Vec<ConversationSummary>, StoreError> {
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
        let n = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        if n == self.fail_on || self.fail_on == usize::MAX {
            return Err((self.error)());
        }
        self.inner.put(user, summary).await
    }
}

/// On AWS a failed attempt is retried. A conversation the failed attempt
/// already wrote holds this upload's messages, so the retry skips it (plan
/// C29) and writes the rest; nothing is added twice.
#[tokio::test]
async fn a_retried_attempt_skips_conversations_the_failed_one_wrote() {
    let base = local_app::memory_stores();
    let mut failing = base.clone();
    failing.conversation_summary_store = Arc::new(RecordsFailingOnce {
        inner: base.conversation_summary_store.clone(),
        fail_on: 2,
        writes: AtomicUsize::new(0),
        error: || StoreError::Backend("the table is down".into()),
    });
    let raw = export(vec![
        conversation(1, "One", &[you(0, "a")]),
        conversation(2, "Two", &[you(0, "b"), claude(1, "c")]),
    ]);
    let upload = put_upload(&failing, raw).await;
    let err = process_upload(&failing, &alice(), upload)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("saving conversation summary 2 of 2"),
        "{err}"
    );
    assert!(!err.is_unusable_file());
    process_upload(&base, &alice(), upload).await.unwrap();
    let records = base
        .conversation_summary_store
        .list_for_user(&alice())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.additions.is_empty()));
    assert_eq!(
        base.user_records
            .get(&alice())
            .await
            .unwrap()
            .totals
            .messages,
        3
    );
}

/// Five lost races in a row fail the attempt with a conflict naming the
/// record, rather than looping for ever.
#[tokio::test]
async fn a_record_that_keeps_losing_races_fails_the_attempt() {
    let mut stores = local_app::memory_stores();
    stores.conversation_summary_store = Arc::new(RecordsFailingOnce {
        inner: stores.conversation_summary_store.clone(),
        fail_on: usize::MAX,
        writes: AtomicUsize::new(0),
        error: || StoreError::Conflict,
    });
    let err = process(
        &stores,
        export(vec![conversation(1, "One", &[you(0, "a")])]),
    )
    .await
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        format!("saving conversation summary 1 of 1 (conversation {}): someone else changed this record first", conv(1))
    );
    let source = std::error::Error::source(&err).unwrap();
    assert!(std::error::Error::source(source).is_none());
}

/// An object store whose writes fail.
struct ObjectsFailing(Arc<dyn ObjectStore>);

#[async_trait]
impl ObjectStore for ObjectsFailing {
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<String, ObjectStoreError> {
        self.0.presign_put(key, ttl).await
    }
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<String, ObjectStoreError> {
        self.0.presign_get(key, ttl).await
    }
    async fn get(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError> {
        self.0.get(key).await
    }
    async fn put(&self, key: &str, data: Vec<u8>) -> Result<(), ObjectStoreError> {
        if key.starts_with("files/") {
            return Err(ObjectStoreError::Backend("the bucket is down".into()));
        }
        self.0.put(key, data).await
    }
    async fn delete(&self, key: &str) -> Result<(), ObjectStoreError> {
        self.0.delete(key).await
    }
}

/// A session store whose writes fail.
struct SessionsFailing(Arc<dyn SessionStore>);

#[async_trait]
impl SessionStore for SessionsFailing {
    async fn list_sessions(&self, user: &UserId) -> Result<Vec<StoredSession>, StoreError> {
        self.0.list_sessions(user).await
    }
    async fn sessions_page(
        &self,
        user: &UserId,
        after: Option<SessionKey>,
        max: usize,
    ) -> Result<Vec<StoredSession>, StoreError> {
        self.0.sessions_page(user, after, max).await
    }
    async fn sessions_of(
        &self,
        user: &UserId,
        id: ConversationId,
    ) -> Result<Vec<StoredSession>, StoreError> {
        self.0.sessions_of(user, id).await
    }
    async fn put_sessions(&self, _: &UserId, _: &[StoredSession]) -> Result<(), StoreError> {
        Err(StoreError::Backend("the table is down".into()))
    }
    async fn delete_sessions(&self, user: &UserId, keys: &[SessionKey]) -> Result<(), StoreError> {
        self.0.delete_sessions(user, keys).await
    }
}

/// A failure in each step of writing a conversation names the step.
#[tokio::test]
async fn a_failed_step_is_named_in_the_error() {
    let mut with_file = conversation(1, "One", &[you(0, "see")]);
    with_file["chat_messages"][0]["attachments"] =
        json!([{"file_name": "a.txt", "extracted_content": "x"}]);
    let raw = export(vec![with_file]);

    let mut files_fail = local_app::memory_stores();
    files_fail.object_store = Arc::new(ObjectsFailing(files_fail.object_store.clone()));
    let err = process(&files_fail, raw.clone()).await.unwrap_err();
    assert!(
        err.to_string()
            .starts_with("saving the files of conversation 1 of 1"),
        "{err}"
    );
    assert!(
        err.to_string()
            .ends_with("object store backend error: the bucket is down"),
        "{err}"
    );
    let source = std::error::Error::source(&err).unwrap();
    assert!(
        std::error::Error::source(source).is_some(),
        "the bucket's own error"
    );

    let mut sessions_fail = local_app::memory_stores();
    sessions_fail.session_store = Arc::new(SessionsFailing(sessions_fail.session_store.clone()));
    let err = process(&sessions_fail, raw).await.unwrap_err();
    assert!(
        err.to_string()
            .starts_with("saving the sessions of conversation 1 of 1"),
        "{err}"
    );
    let source = std::error::Error::source(&err).unwrap();
    assert!(
        std::error::Error::source(source).is_some(),
        "the backend's own error"
    );
}

/// The progress row is written while the file is still being parsed, and
/// while conversations are written; a progress write that fails is logged
/// and processing goes on (§8b).
#[tokio::test]
async fn progress_is_written_while_parsing_and_a_failed_write_doesnt_stop_processing() {
    let stores = local_app::memory_stores();
    let raw = export(vec![
        conversation(1, "One", &[you(0, "a")]),
        conversation(2, "Two", &[you(0, "b")]),
    ]);
    let upload = put_upload(&stores, raw.clone()).await;
    process_upload_reporting(&stores, &alice(), upload, Duration::ZERO)
        .await
        .unwrap();
    let progress = stores
        .upload_outcome_store
        .get_progress(&alice(), upload)
        .await
        .unwrap()
        .unwrap();
    let written = progress.processing.unwrap();
    assert_eq!(
        (written.conversations_written, written.conversations_total),
        (2, 2)
    );

    // The same, with every progress write failing.
    let base = local_app::memory_stores();
    let mut stores = base.clone();
    stores.upload_outcome_store = Arc::new(ProgressFails(base.upload_outcome_store.clone()));
    let upload = put_upload(&stores, raw).await;
    process_upload_reporting(&stores, &alice(), upload, Duration::ZERO)
        .await
        .unwrap();
    assert!(matches!(
        base.upload_outcome_store
            .get_outcome(&alice(), upload)
            .await
            .unwrap(),
        Some(UploadOutcome::Ready { .. })
    ));
}

struct ProgressFails(Arc<dyn timeline_core::ports::uploads::UploadOutcomeStore>);

#[async_trait]
impl timeline_core::ports::uploads::UploadOutcomeStore for ProgressFails {
    async fn record_outcome(
        &self,
        u: &UserId,
        id: UploadId,
        o: UploadOutcome,
    ) -> Result<(), StoreError> {
        self.0.record_outcome(u, id, o).await
    }
    async fn get_outcome(
        &self,
        u: &UserId,
        id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError> {
        self.0.get_outcome(u, id).await
    }
    async fn record_attempt(&self, u: &UserId, id: UploadId) -> Result<usize, StoreError> {
        self.0.record_attempt(u, id).await
    }
    async fn record_attempt_error(
        &self,
        u: &UserId,
        id: UploadId,
        e: String,
    ) -> Result<(), StoreError> {
        self.0.record_attempt_error(u, id, e).await
    }
    async fn record_processing_progress(
        &self,
        _: &UserId,
        _: UploadId,
        _: timeline_core::ports::uploads::ProcessingProgress,
    ) -> Result<(), StoreError> {
        Err(StoreError::Backend("progress is down".into()))
    }
    async fn get_progress(
        &self,
        u: &UserId,
        id: UploadId,
    ) -> Result<Option<timeline_core::ports::uploads::UploadProgress>, StoreError> {
        self.0.get_progress(u, id).await
    }
    async fn record_received(
        &self,
        u: &UserId,
        id: UploadId,
        f: timeline_core::conversation_metadata::UploadFacts,
    ) -> Result<(), StoreError> {
        self.0.record_received(u, id, f).await
    }
    async fn get_received(
        &self,
        u: &UserId,
        id: UploadId,
    ) -> Result<Option<timeline_core::conversation_metadata::UploadFacts>, StoreError> {
        self.0.get_received(u, id).await
    }
}

/// A store failure reading the outcome is a storage error, retried, with
/// the store's error as its source; each other error names its cause.
#[tokio::test]
async fn each_processing_error_carries_its_cause() {
    struct OutcomesDown;
    #[async_trait]
    impl timeline_core::ports::uploads::UploadOutcomeStore for OutcomesDown {
        async fn record_outcome(
            &self,
            _: &UserId,
            _: UploadId,
            _: UploadOutcome,
        ) -> Result<(), StoreError> {
            Err(StoreError::Backend("down".into()))
        }
        async fn get_outcome(
            &self,
            _: &UserId,
            _: UploadId,
        ) -> Result<Option<UploadOutcome>, StoreError> {
            Err(StoreError::Backend("down".into()))
        }
        async fn record_attempt(&self, _: &UserId, _: UploadId) -> Result<usize, StoreError> {
            unreachable!("not used by process_upload")
        }
        async fn record_attempt_error(
            &self,
            _: &UserId,
            _: UploadId,
            _: String,
        ) -> Result<(), StoreError> {
            unreachable!("not used by process_upload")
        }
        async fn record_processing_progress(
            &self,
            _: &UserId,
            _: UploadId,
            _: timeline_core::ports::uploads::ProcessingProgress,
        ) -> Result<(), StoreError> {
            unreachable!("not reached before the outcome is read")
        }
        async fn get_progress(
            &self,
            _: &UserId,
            _: UploadId,
        ) -> Result<Option<timeline_core::ports::uploads::UploadProgress>, StoreError> {
            unreachable!("not used by process_upload")
        }
        async fn record_received(
            &self,
            _: &UserId,
            _: UploadId,
            _: timeline_core::conversation_metadata::UploadFacts,
        ) -> Result<(), StoreError> {
            unreachable!("not used by process_upload")
        }
        async fn get_received(
            &self,
            _: &UserId,
            _: UploadId,
        ) -> Result<Option<timeline_core::conversation_metadata::UploadFacts>, StoreError> {
            unreachable!("not reached before the outcome is read")
        }
    }
    let mut stores = local_app::memory_stores();
    stores.upload_outcome_store = Arc::new(OutcomesDown);
    let err = process_upload(&stores, &alice(), UploadId(uuid::Uuid::nil()))
        .await
        .unwrap_err();
    assert!(matches!(err, ProcessingError::Store(_)), "{err:?}");
    assert_eq!(err.to_string(), "storage backend error: down");
    assert!(std::error::Error::source(&err).is_some());

    let stores = local_app::memory_stores();
    let missing = process_upload(&stores, &alice(), UploadId(uuid::Uuid::nil()))
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        ProcessingError::ObjectStore(ObjectStoreError::NotFound)
    ));
    assert_eq!(missing.to_string(), "object not found");
    assert!(std::error::Error::source(&missing).is_some());

    let review = export(vec![{
        let mut c = conversation(1, "One", &[you(0, "a")]);
        c["chat_messages"][0]["_claude_timeline_user"] = json!(7);
        c
    }]);
    let err = process(&stores, review).await.unwrap_err();
    assert!(matches!(err, ProcessingError::ReviewField { .. }));
    assert!(std::error::Error::source(&err).is_some());
    let bad = process(&stores, "[1".to_string()).await.unwrap_err();
    assert!(std::error::Error::source(&bad).is_some());
    let not_utf8 = put_upload(&stores, String::new()).await;
    stores
        .object_store
        .put(&raw_object_key(&alice(), not_utf8), vec![0xff])
        .await
        .unwrap();
    let err = process_upload(&stores, &alice(), not_utf8)
        .await
        .unwrap_err();
    assert!(std::error::Error::source(&err).is_some());
    assert!(
        err.to_string()
            .starts_with("uploaded file was not valid UTF-8"),
        "{err}"
    );
}

/// A later file's messages of unknown time aren't added to a known
/// conversation; the run's log line counts them (§4e).
#[tokio::test]
async fn messages_of_unknown_time_a_later_file_holds_are_counted_as_skipped() {
    let stores = local_app::memory_stores();
    let first = conversation(1, "One", &[you(0, "a")]);
    process(&stores, export(vec![first.clone()])).await.unwrap();
    let mut later = first;
    later["chat_messages"].as_array_mut().unwrap().push(json!({
        "uuid": msg(1, 5), "parent_message_uuid": msg(1, 1), "sender": "assistant",
        "content": [{"type": "text", "text": "when?"}],
    }));
    let (result, record) = recording(process(&stores, export(vec![later]))).await;
    result.unwrap();
    assert_eq!(
        record.facts["messages_of_unknown_time_skipped"], 1,
        "{:?}",
        record.facts
    );
}

/// The download writes text pieces and leaves file marks out of a
/// message's content, naming the files in `files` instead.
#[tokio::test]
async fn the_download_names_presented_files_and_leaves_their_marks_out() {
    let router = local_app::router();
    let mut c = conversation(1, "One", &[you(0, "make it"), claude(1, "done")]);
    c["chat_messages"][1]["content"] = json!([
        {"type": "tool_use", "name": "create_file", "input": {"path": "/o/a.md", "file_text": "# A"}},
        {"type": "tool_use", "name": "present_files", "input": {"filepaths": ["/o/a.md"]}},
        {"type": "text", "text": "Here."},
    ]);
    let token = local_app::signed_in_with(&router, "alice", &export(vec![c])).await;
    let file = local_app::exported(&router, &token).await;
    let reply = &file["conversations"][0]["chat_messages"][1];
    assert_eq!(
        reply["content"],
        json!([{"type": "text", "text": "Here.", "citations": []}])
    );
    assert_eq!(reply["files"], json!([{"file_name": "a.md"}]));
}

/// Editing a file's details redoes a record someone else changed first, and
/// gives up with a server error after five lost races; another storage
/// failure is a server error at once.
#[tokio::test]
async fn a_details_edit_redoes_lost_races_and_reports_failures() {
    for (fail_on, error, expected) in [
        (
            1,
            (|| StoreError::Conflict) as fn() -> StoreError,
            axum::http::StatusCode::OK,
        ),
        (
            usize::MAX,
            || StoreError::Conflict,
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        ),
        (
            1,
            || StoreError::Backend("down".into()),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ] {
        let (_, mut state, dev) = local_app::app();
        let healthy = timeline_api::app::build_router(state.clone())
            .merge(timeline_api::app::build_dev_router(dev.clone()));
        let token = local_app::signed_in_with(
            &healthy,
            "alice",
            &export(vec![conversation(1, "One", &[you(0, "a")])]),
        )
        .await;
        state.conversation_summary_store = Arc::new(RecordsFailingOnce {
            inner: state.conversation_summary_store.clone(),
            fail_on,
            writes: AtomicUsize::new(0),
            error,
        });
        let router = timeline_api::app::build_router(state);
        let (status, body) = local_app::send(
            &router,
            local_app::request(
                "PUT",
                &format!("/conversations/{}/metadata", conv(1)),
                &token,
                Some(json!({"medium": {"kind": "typed"}})),
            ),
        )
        .await;
        assert_eq!(status, expected, "{body}");
    }
}

/// A merge whose record write loses a race is redone; one whose record
/// write fails reports it, naming the record.
#[tokio::test]
async fn a_merge_that_loses_its_race_is_redone_and_a_failed_one_reported() {
    for (error, ok) in [
        ((|| StoreError::Conflict) as fn() -> StoreError, true),
        (|| StoreError::Backend("down".into()), false),
    ] {
        let base = local_app::memory_stores();
        let first = conversation(1, "One", &[you(0, "a")]);
        process(&base, export(vec![first.clone()])).await.unwrap();
        let mut stores = base.clone();
        stores.conversation_summary_store = Arc::new(RecordsFailingOnce {
            inner: base.conversation_summary_store.clone(),
            fail_on: 1,
            writes: AtomicUsize::new(0),
            error,
        });
        let mut later = first;
        later["chat_messages"]
            .as_array_mut()
            .unwrap()
            .push(human(2, 1, 90, "b"));
        let result = process(&stores, export(vec![later])).await;
        let record = base
            .conversation_summary_store
            .get(&alice(), ConversationId(conv(1).parse().unwrap()))
            .await
            .unwrap()
            .unwrap();
        if ok {
            result.unwrap();
            assert_eq!(record.message_count, 2);
        } else {
            let err = result.unwrap_err();
            assert!(
                err.to_string()
                    .starts_with("saving conversation summary 1 of 1"),
                "{err}"
            );
            assert_eq!(record.message_count, 1);
        }
    }
}
