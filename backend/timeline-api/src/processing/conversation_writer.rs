//! Writing one kept conversation (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7): its files,
//! then its message rows, then its sessions, and its record last, written
//! only if the record's version is still the one read (C1). Another file of
//! the same batch processed at the same moment can win that race; then the
//! conversation is read again and redone as a merge.
//!
//! A merge computes the record's counts and span from the rows actually
//! stored, so a redo after a lost race counts every row, including any the
//! losing attempt wrote before it lost.

use std::collections::{BTreeSet, HashSet};
use std::time::{Duration, Instant};

use timeline_core::conversation_metadata::{
    guess_summary, span_of_times, ConversationSpan, MetadataOrigin, UploadFacts,
};
use timeline_core::keep::{Kept, KeptConversation, KeptFile};
use timeline_core::merge::{plan_merge, MergePlan};
use timeline_core::model::{Conversation, ConversationId, MessageId};
use timeline_core::ports::conversations::ConversationSummary;
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::messages::EntryRange;
use timeline_core::ports::uploads::file_object_key;
use timeline_core::stored_message::{count_out_of_order, Entry, EntryKey, FileContents};
use timeline_core::stored_session::{sessions_for, SessionKey, StoredSession};
use timeline_core::MessageTime;

use crate::s3_trigger::ProcessingStores;

/// How often a conversation is redone after losing a race, before the
/// attempt fails.
const REDOS: usize = 5;

/// Time spent in each step, summed over the conversations written (several
/// are written at once, so the sums can exceed the run's own time).
#[derive(Debug, Default, Clone, Copy)]
pub struct StepTimes {
    pub files: Duration,
    pub rows: Duration,
    pub sessions: Duration,
    pub record: Duration,
}

impl StepTimes {
    pub fn add(&mut self, other: StepTimes) {
        self.files += other.files;
        self.rows += other.rows;
        self.sessions += other.sessions;
        self.record += other.record;
    }
}

/// What writing one conversation did.
#[derive(Debug, Default)]
pub struct Written {
    /// This conversation and any branches kept as their own.
    pub conversation_ids: Vec<ConversationId>,
    pub times: StepTimes,
    /// The conversation was stored before this file.
    pub already_present: bool,
    /// It was stored before and gained rows.
    pub gained: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
    /// After the merge it holds fewer messages than this file's copy (plan
    /// C18 of the screen-flow plan).
    pub fewer_than_file: bool,
    pub untimed_skipped: usize,
}

/// Which step of writing a conversation failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Files,
    Rows,
    Sessions,
    Record,
}

/// What failed: a table or the file store.
#[derive(Debug)]
pub enum WriteFailure {
    Store(StoreError),
    Object(ObjectStoreError),
}

/// A failure writing one conversation: the step, and its cause.
#[derive(Debug)]
pub struct WriteError {
    pub step: Step,
    pub failure: WriteFailure,
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WriteFailure::Store(e) => write!(f, "{e}"),
            WriteFailure::Object(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WriteFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            WriteFailure::Store(e) => e.source(),
            WriteFailure::Object(e) => e.source(),
        }
    }
}

/// Maps a store's failure to a failure of `step`.
fn at<E: Into<WriteFailure>>(step: Step) -> impl FnOnce(E) -> WriteError {
    move |e| WriteError {
        step,
        failure: e.into(),
    }
}

impl From<StoreError> for WriteFailure {
    fn from(e: StoreError) -> Self {
        WriteFailure::Store(e)
    }
}

impl From<ObjectStoreError> for WriteFailure {
    fn from(e: ObjectStoreError) -> Self {
        WriteFailure::Object(e)
    }
}

/// The span of the known times of `entries`' messages.
fn known_span(entries: &[Entry]) -> Option<ConversationSpan> {
    span_of_times(
        entries
            .iter()
            .filter_map(Entry::as_message)
            .filter_map(|m| m.key.time().known()),
    )
}

async fn store_files(
    stores: &ProcessingStores,
    user_id: &UserId,
    conversation_id: ConversationId,
    files: &[KeptFile],
) -> Result<(), WriteError> {
    for file in files {
        let key = file_object_key(user_id, conversation_id, file.message_id, file.number);
        stores
            .object_store
            .put(&key, file.text.clone().into_bytes())
            .await
            .map_err(at(Step::Files))?;
    }
    Ok(())
}

/// Writes the files, rows and sessions of a conversation stored for the
/// first time.
async fn write_rows_and_sessions(
    stores: &ProcessingStores,
    user_id: &UserId,
    conversation: &KeptConversation,
    span: &ConversationSpan,
    times: &mut StepTimes,
) -> Result<(), WriteError> {
    let started = Instant::now();
    store_files(
        stores,
        user_id,
        conversation.conversation_id,
        &conversation.files,
    )
    .await?;
    times.files += started.elapsed();
    let started = Instant::now();
    stores
        .message_writer
        .put_entries(user_id, &conversation.entries)
        .await
        .map_err(at(Step::Rows))?;
    times.rows += started.elapsed();
    let started = Instant::now();
    let sessions = sessions_for(conversation.conversation_id, &conversation.entries, span);
    stores
        .session_store
        .put_sessions(user_id, &sessions)
        .await
        .map_err(at(Step::Sessions))?;
    times.sessions += started.elapsed();
    Ok(())
}

/// A new conversation's record, guessed from its kept messages.
fn new_record(
    conversation: &KeptConversation,
    upload_id: UploadId,
    facts: &UploadFacts,
) -> ConversationSummary {
    let mut record = guess_summary(
        &Conversation {
            uuid: conversation.conversation_id,
            name: conversation.name.clone(),
            chat_messages: conversation.kept_path.clone(),
            extra: serde_json::Map::new(),
        },
        upload_id,
        facts,
    );
    record.out_of_order = count_out_of_order(&conversation.entries);
    record
}

/// Writes a conversation never stored before; `Ok(false)` when someone else
/// stored it first (a lost race), so it must be redone as a merge.
async fn write_new(
    stores: &ProcessingStores,
    user_id: &UserId,
    conversation: &KeptConversation,
    record: ConversationSummary,
    times: &mut StepTimes,
) -> Result<bool, WriteError> {
    write_rows_and_sessions(stores, user_id, conversation, &record.span, times).await?;
    let started = Instant::now();
    let result = stores.conversation_summary_store.put(user_id, record).await;
    times.record += started.elapsed();
    match result {
        Ok(_) => Ok(true),
        Err(StoreError::Conflict) => Ok(false),
        Err(e) => Err(at(Step::Record)(e)),
    }
}

/// Writes one conversation of the upload; see the module doc.
pub async fn write_conversation(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    facts: &UploadFacts,
    kept: &Kept,
) -> Result<Written, WriteError> {
    let id = kept.main.conversation_id;
    let mut written = Written::default();
    for _ in 0..REDOS {
        let stored = stores
            .conversation_summary_store
            .get(user_id, id)
            .await
            .map_err(at(Step::Record))?;
        let done = match stored {
            None => {
                let mut record = new_record(&kept.main, upload_id, facts);
                record.branches = kept
                    .branches
                    .iter()
                    .map(|b| b.conversation.conversation_id)
                    .collect();
                let stored =
                    write_new(stores, user_id, &kept.main, record, &mut written.times).await?;
                if stored {
                    for branch in &kept.branches {
                        write_branch(
                            stores,
                            user_id,
                            upload_id,
                            facts,
                            &branch.conversation,
                            branch.branch_of,
                            &mut written.times,
                        )
                        .await?;
                        written
                            .conversation_ids
                            .push(branch.conversation.conversation_id);
                    }
                }
                stored
            }
            Some(record)
                if record.source.upload_id == upload_id
                    || record.additions.contains(&upload_id) =>
            {
                // A repeat of an earlier attempt of this same upload,
                // retried on AWS: its messages are already in.
                written.already_present = true;
                true
            }
            Some(record) => {
                written.already_present = true;
                merge(
                    stores,
                    user_id,
                    upload_id,
                    facts,
                    kept,
                    record,
                    &mut written,
                )
                .await?
            }
        };
        if done {
            written.conversation_ids.insert(0, id);
            return Ok(written);
        }
    }
    Err(at(Step::Record)(StoreError::Conflict))
}

/// A branch kept as a conversation of its own: written new, unless an
/// earlier attempt already wrote it (its id is made from the branch, so it
/// is the same).
async fn write_branch(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    facts: &UploadFacts,
    conversation: &KeptConversation,
    branch_of: ConversationId,
    times: &mut StepTimes,
) -> Result<(), WriteError> {
    let mut record = new_record(conversation, upload_id, facts);
    record.branch_of = Some(branch_of);
    write_new(stores, user_id, conversation, record, times).await?;
    Ok(())
}

/// Merges `kept` into the stored conversation `record`; `Ok(false)` when
/// someone else changed the record first.
async fn merge(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    facts: &UploadFacts,
    kept: &Kept,
    mut record: ConversationSummary,
    written: &mut Written,
) -> Result<bool, WriteError> {
    let id = record.conversation_id;
    let stored = stores
        .message_reader
        .read_entries(
            user_id,
            EntryRange {
                conversation_id: id,
                positions: None,
                after: None,
            },
        )
        .await
        .map_err(at(Step::Rows))?;
    let plan = plan_merge(&stored, record.message_span.as_ref(), &record.name, kept);
    written.untimed_skipped = plan.untimed_skipped;
    let stored_messages = stored.iter().filter(|e| e.as_message().is_some()).count();
    // Nothing to add, and the record already counts every row: done. When
    // the rows hold more than the record counts (an attempt that lost a
    // race wrote rows before it lost), the record is brought into line
    // below even though this file adds nothing more.
    if plan.changes_nothing() && stored_messages == record.message_count {
        written.fewer_than_file = stored_messages < kept.main.kept_path.len();
        return Ok(true);
    }
    let old_sessions = stores
        .session_store
        .sessions_of(user_id, id)
        .await
        .map_err(at(Step::Sessions))?;

    // Rows a revived branch replaced go first: its note takes the first
    // one's key.
    let mut times = StepTimes::default();
    let started = Instant::now();
    if let Some(moved) = &plan.moved {
        move_files(stores, user_id, id, moved.conversation_id, &moved.entries).await?;
    }
    store_files(stores, user_id, id, &plan.files).await?;
    times.files += started.elapsed();
    let started = Instant::now();
    if !plan.remove.is_empty() {
        stores
            .message_writer
            .delete_entries(user_id, &plan.remove)
            .await
            .map_err(at(Step::Rows))?;
    }
    stores
        .message_writer
        .put_entries(user_id, &plan.add)
        .await
        .map_err(at(Step::Rows))?;
    times.rows += started.elapsed();

    let after = entries_after(&stored, &plan);
    let added_span = known_span(&plan.add);
    record.message_count = after.iter().filter(|e| e.as_message().is_some()).count();
    record.untimed = after
        .iter()
        .filter_map(Entry::as_message)
        .filter(|m| m.key.time() == MessageTime::Unknown)
        .count();
    record.out_of_order = count_out_of_order(&after);
    record.message_span = known_span(&after);
    if record.span_origin == MetadataOrigin::Guessed {
        if let Some(span) = &record.message_span {
            record.span = record.span.widened_to(span);
        }
    }
    record.additions.push(upload_id);
    for branch in &plan.branches {
        record.branches.push(branch.conversation.conversation_id);
    }
    if let Some(moved) = &plan.moved {
        record.branches.push(moved.conversation_id);
    }

    let started = Instant::now();
    let sessions = sessions_for(id, &after, &record.span);
    stores
        .session_store
        .put_sessions(user_id, &sessions)
        .await
        .map_err(at(Step::Sessions))?;
    let stale: Vec<SessionKey> = old_sessions
        .iter()
        .map(StoredSession::key)
        .filter(|k| k.number >= sessions.len())
        .collect();
    if !stale.is_empty() {
        stores
            .session_store
            .delete_sessions(user_id, &stale)
            .await
            .map_err(at(Step::Sessions))?;
    }
    times.sessions += started.elapsed();

    let started = Instant::now();
    let put = stores
        .conversation_summary_store
        .put(user_id, record.clone())
        .await;
    times.record += started.elapsed();
    written.times.add(times);
    match put {
        Ok(_) => {}
        Err(StoreError::Conflict) => return Ok(false),
        Err(e) => return Err(at(Step::Record)(e)),
    }

    for branch in &plan.branches {
        write_branch(
            stores,
            user_id,
            upload_id,
            facts,
            &branch.conversation,
            id,
            &mut written.times,
        )
        .await?;
        written
            .conversation_ids
            .push(branch.conversation.conversation_id);
    }
    if let Some(moved) = &plan.moved {
        let conversation = KeptConversation {
            conversation_id: moved.conversation_id,
            name: moved.name.clone(),
            entries: moved.entries.clone(),
            files: Vec::new(),
            kept_path: Vec::new(),
        };
        let mut moved_record = new_record(&conversation, upload_id, facts);
        moved_record.message_count = moved.entries.len();
        moved_record.untimed = moved
            .entries
            .iter()
            .filter(|e| e.key().time() == MessageTime::Unknown)
            .count();
        moved_record.out_of_order = count_out_of_order(&moved.entries);
        moved_record.message_span = known_span(&moved.entries);
        if let Some(span) = moved_record.message_span {
            moved_record.span = span;
        }
        moved_record.branch_of = Some(id);
        write_new(
            stores,
            user_id,
            &conversation,
            moved_record,
            &mut written.times,
        )
        .await?;
        written.conversation_ids.push(moved.conversation_id);
    }
    written.gained = added_span.map(|s| {
        (
            s.start().with_timezone(&chrono::Utc),
            s.end().with_timezone(&chrono::Utc),
        )
    });
    written.fewer_than_file =
        after.iter().filter(|e| e.as_message().is_some()).count() < kept.main.kept_path.len();
    Ok(true)
}

/// The conversation's rows once `plan` is applied to `stored`.
fn entries_after(stored: &[Entry], plan: &MergePlan) -> Vec<Entry> {
    let removed: HashSet<EntryKey> = plan.remove.iter().copied().collect();
    let added: BTreeSet<EntryKey> = plan.add.iter().map(Entry::key).collect();
    let mut after: Vec<Entry> = stored
        .iter()
        .filter(|e| !removed.contains(&e.key()) && !added.contains(&e.key()))
        .cloned()
        .collect();
    after.extend(plan.add.iter().cloned());
    after.sort_by_key(Entry::key);
    after
}

/// Copies the stored files of messages moved to another conversation to
/// their new keys, and removes the old ones.
async fn move_files(
    stores: &ProcessingStores,
    user_id: &UserId,
    from: ConversationId,
    to: ConversationId,
    entries: &[Entry],
) -> Result<(), WriteError> {
    for message in entries.iter().filter_map(Entry::as_message) {
        let id: MessageId = message.key.id;
        for file in message
            .files()
            .filter(|f| f.contents == FileContents::Stored)
        {
            let old = file_object_key(user_id, from, id, file.number);
            let objects = stores.object_store.as_ref();
            let bytes = objects.get(&old).await.map_err(at(Step::Files))?;
            objects
                .put(&file_object_key(user_id, to, id, file.number), bytes)
                .await
                .map_err(at(Step::Files))?;
            objects.delete(&old).await.map_err(at(Step::Files))?;
        }
    }
    Ok(())
}
