//! Turns an uploaded `conversations.json` into stored rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7). One
//! orchestration function, used by both callers the migration plan's §V2a
//! describes: the S3-triggered Lambda in production
//! (`src/bin/process_upload.rs`) and the local `_dev/local-storage` route.
//!
//! After the file is read and parsed (as a stream, one conversation at a
//! time, [`upload_reader`]), processing:
//! 1. cuts each new or extended conversation into sessions;
//! 2. stores the files of §4;
//! 3. writes the message rows, then each new or changed session;
//! 4. writes each conversation's record last, with its new version
//!    ([`conversation_writer`]);
//! 5. raises the user's data version and totals;
//! 6. records the outcome and deletes the uploaded file. A failed attempt
//!    leaves it, so a retry still has it.
//!
//! An upload already processed (its outcome is ready) is skipped, apart from
//! deleting the file if it is still there: the original is deleted once
//! processed, so a repeated S3 event would otherwise fail and overwrite
//! "ready" with "failed".
//!
//! While it runs, how far it has got is written to the upload's progress
//! row every second (§8b): bytes of the file parsed, then conversations
//! written. The run's log line records how long each step took.
//!
//! **This does not compute flags.** Detection is a separate, user-triggered
//! pass (`POST /detect`, see `crate::routes::detect`).

pub mod conversation_writer;
pub mod upload_reader;

use std::fmt;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::stream::{self, StreamExt};
use timeline_core::flag_values::FlagSet;
use timeline_core::flags::anger::detect_angry;
use timeline_core::flags::caps::has_emphasis_caps;
use timeline_core::flags::criticism::detect_critical;
use timeline_core::keep::Kept;
use timeline_core::model::{ConversationId, MessageId};
use timeline_core::ports::errors::{ObjectStoreError, StoreError};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::{raw_object_key, ProcessingProgress, UploadOutcome};
use timeline_core::ports::user_record::Totals;
use timeline_core::stored_message::Entry;
use timeline_core::FormatError;

use self::conversation_writer::{write_conversation, Step, StepTimes, WriteError, Written};
use self::upload_reader::{decompress, read_conversations, ReadError};
use crate::request_record::note;
use crate::s3_trigger::ProcessingStores;

/// Conversations written at the same time. Each sends its rows in parallel
/// batches too (`timeline_storage::dynamo`), so this mostly overlaps the
/// round trips of small conversations.
const CONVERSATIONS_AT_ONCE: usize = 8;

/// How often the progress row is written while processing runs (§8b).
pub const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// The largest row kept, in bytes of its JSON: DynamoDB refuses rows over
/// 400 KB, and a row carries its key and flags besides.
pub const LARGEST_ROW: usize = 390_000;

#[derive(Debug)]
pub enum ProcessingError {
    /// `POST /uploads` never recorded this upload, so its file name, upload
    /// time and human name are unknown. Retrying can't help.
    NoUploadRecord,
    RawObjectNotUtf8(std::str::Utf8Error),
    /// It started like a gzip file but didn't decompress.
    Decompress(std::io::Error),
    Format(FormatError),
    Store(StoreError),
    ObjectStore(ObjectStoreError),
    /// A message's `_claude_timeline_user` field is not a review this
    /// project wrote: not an object of optional caps/critical/angry booleans.
    ReviewField {
        message_id: MessageId,
        error: serde_json::Error,
    },
    /// A message too large for one stored row (§8b).
    MessageTooLarge {
        conversation_id: ConversationId,
        message_id: MessageId,
        bytes: usize,
    },
    /// The `FailProcessing` test setting is on: the attempt fails before
    /// reading the file (plan `2026-10-02-upload-processing-failures.md`
    /// §2b). Retried like a storage failure.
    FailingOnPurpose,
    /// Storing one conversation failed: its rows, files, sessions or record.
    /// Numbered from 1 in the order they were read, so a log line says
    /// which (plan `2026-10-02-upload-processing-failures.md` §1a). Rows
    /// still unwritten after every retry are named by count in `source`.
    SavingConversation {
        number: usize,
        total: usize,
        conversation_id: ConversationId,
        source: WriteError,
    },
}

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessingError::NoUploadRecord => {
                write!(
                    f,
                    "there is no record of this upload being started (POST /uploads)"
                )
            }
            ProcessingError::RawObjectNotUtf8(e) => {
                write!(f, "uploaded file was not valid UTF-8: {e}")
            }
            ProcessingError::Decompress(e) => {
                write!(f, "the compressed file could not be decompressed: {e}")
            }
            ProcessingError::Format(e) => write!(f, "{e}"),
            ProcessingError::Store(e) => write!(f, "{e}"),
            ProcessingError::ObjectStore(e) => write!(f, "{e}"),
            ProcessingError::ReviewField { message_id, error } => {
                write!(
                    f,
                    "message {message_id} has an unreadable _claude_timeline_user review: {error}"
                )
            }
            ProcessingError::MessageTooLarge {
                conversation_id,
                message_id,
                bytes,
            } => write!(
                f,
                "message {message_id} in conversation {conversation_id} is too large to store \
                 ({bytes} bytes; the most one stored message can hold is {LARGEST_ROW})"
            ),
            ProcessingError::FailingOnPurpose => {
                write!(f, "failing on purpose (FailProcessing is on)")
            }
            ProcessingError::SavingConversation {
                number,
                total,
                conversation_id,
                source,
            } => {
                let what = match source.step {
                    Step::Files => "the files of conversation",
                    Step::Rows => "the message rows of conversation",
                    Step::Sessions => "the sessions of conversation",
                    Step::Record => "conversation summary",
                };
                write!(
                    f,
                    "saving {what} {number} of {total} (conversation {conversation_id}): {}",
                    source.failure
                )
            }
        }
    }
}

impl std::error::Error for ProcessingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProcessingError::NoUploadRecord
            | ProcessingError::FailingOnPurpose
            | ProcessingError::MessageTooLarge { .. } => None,
            ProcessingError::RawObjectNotUtf8(e) => Some(e),
            ProcessingError::Decompress(e) => Some(e),
            ProcessingError::Format(e) => Some(e),
            ProcessingError::Store(e) => Some(e),
            ProcessingError::ObjectStore(e) => Some(e),
            ProcessingError::ReviewField { error, .. } => Some(error),
            ProcessingError::SavingConversation { source, .. } => Some(&source.failure),
        }
    }
}

impl From<StoreError> for ProcessingError {
    fn from(e: StoreError) -> Self {
        ProcessingError::Store(e)
    }
}

impl From<ObjectStoreError> for ProcessingError {
    fn from(e: ObjectStoreError) -> Self {
        ProcessingError::ObjectStore(e)
    }
}

impl From<ReadError> for ProcessingError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Decompress(e) => ProcessingError::Decompress(e),
            ReadError::NotUtf8(e) => ProcessingError::RawObjectNotUtf8(e),
            ReadError::Format(e) => ProcessingError::Format(e),
            ReadError::Keep(timeline_core::keep::KeepError::Review { message_id, error }) => {
                ProcessingError::ReviewField { message_id, error }
            }
        }
    }
}

impl ProcessingError {
    /// Whether the file itself is unusable, so retrying can't help: it is
    /// recorded as failed for the page, and not retried.
    pub fn is_unusable_file(&self) -> bool {
        matches!(
            self,
            ProcessingError::NoUploadRecord
                | ProcessingError::RawObjectNotUtf8(_)
                | ProcessingError::Decompress(_)
                | ProcessingError::Format(_)
                | ProcessingError::ReviewField { .. }
                | ProcessingError::MessageTooLarge { .. }
        )
    }
}

/// The non-generative pass: dictionary-checked ALL-CAPS emphasis plus
/// keyword/sentiment criticism and anger. Run by the scan
/// (`routes::detect`).
pub fn heuristic_flags(text: &str) -> FlagSet {
    FlagSet {
        caps: has_emphasis_caps(text),
        critical: detect_critical(text),
        angry: detect_angry(text),
    }
}

fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

/// Writes the progress row, logging rather than failing when it can't:
/// progress is shown to the page but is not part of the result.
async fn report_progress(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    progress: ProcessingProgress,
) {
    if let Err(e) = stores
        .upload_outcome_store
        .record_processing_progress(user_id, upload_id, progress)
        .await
    {
        eprintln!("upload {upload_id}: couldn't record processing progress: {e}");
    }
}

/// Processes one upload; see the module doc. A file that can't be used is
/// recorded as `Failed` before the error is returned, so callers never need
/// to record failure themselves.
pub async fn process_upload(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
) -> Result<(), ProcessingError> {
    process_upload_reporting(stores, user_id, upload_id, PROGRESS_EVERY).await
}

/// [`process_upload`], writing the progress row every `every` rather than
/// every [`PROGRESS_EVERY`]: as with the request time limit (plan §8c), the
/// interval is handed in, so a test can report at every step whatever the
/// computer's speed.
pub async fn process_upload_reporting(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    every: Duration,
) -> Result<(), ProcessingError> {
    let result = process(stores, user_id, upload_id, every).await;
    if let Err(e) = &result {
        if e.is_unusable_file() {
            stores
                .upload_outcome_store
                .record_outcome(
                    user_id,
                    upload_id,
                    UploadOutcome::Failed {
                        reason: e.to_string(),
                    },
                )
                .await?;
        }
    }
    result
}

async fn process(
    stores: &ProcessingStores,
    user_id: &UserId,
    upload_id: UploadId,
    every: Duration,
) -> Result<(), ProcessingError> {
    let key = raw_object_key(user_id, upload_id);
    if let Some(UploadOutcome::Ready { .. }) = stores
        .upload_outcome_store
        .get_outcome(user_id, upload_id)
        .await?
    {
        note("skipped", "already processed");
        stores.object_store.delete(&key).await?;
        return Ok(());
    }

    let started = Instant::now();
    let raw = stores.object_store.get(&key).await?;
    note("ms_read", ms(started.elapsed()));
    // For the run's log line (crate::s3_trigger); nothing outside a
    // recording sees these (crate::request_record).
    note("bytes", raw.len());
    let started = Instant::now();
    let plain = decompress(raw)?;
    note("ms_decompress", ms(started.elapsed()));
    note("bytes_plain", plain.len());

    let bytes_total = plain.len() as u64;
    let bytes_read = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let parse = {
        let bytes_read = bytes_read.clone();
        tokio::task::spawn_blocking(move || read_conversations(&plain, bytes_read))
    };
    tokio::pin!(parse);
    let kept = loop {
        tokio::select! {
            joined = &mut parse => break joined,
            () = tokio::time::sleep(every) => {
                report_progress(stores, user_id, upload_id, ProcessingProgress {
                    bytes_read: bytes_read.load(Ordering::Relaxed),
                    bytes_total,
                    ..ProcessingProgress::default()
                }).await;
            }
        }
    };
    // Unreachable backstop: the parse task returns rather than panics.
    let kept: Vec<Kept> = kept.expect("the parse task doesn't panic")?;
    note("ms_parse", ms(started.elapsed()));
    refuse_oversized(&kept)?;
    note("reviews", reviews_in(&kept));

    // Read only now, after the file has been read and checked, so a file
    // that hasn't arrived, or isn't an export, is reported as that rather
    // than as a missing record.
    let Some(facts) = stores
        .upload_outcome_store
        .get_received(user_id, upload_id)
        .await?
    else {
        return Err(ProcessingError::NoUploadRecord);
    };

    let total = kept.len();
    note("conversations", total);
    let written_count = AtomicUsize::new(0);
    let times = Mutex::new(StepTimes::default());
    let each = Each {
        stores,
        user_id,
        upload_id,
        facts: &facts,
        total,
        written_count: &written_count,
        times: &times,
    };
    let futures: Vec<_> = kept
        .iter()
        .enumerate()
        .map(|(index, conversation)| write_one(each, index, conversation))
        .collect();
    let writes = stream::iter(futures).buffered(CONVERSATIONS_AT_ONCE);
    tokio::pin!(writes);
    let mut all = Vec::with_capacity(total);
    let mut last_report = Instant::now();
    while let Some(result) = writes.next().await {
        all.push(result?);
        if last_report.elapsed() >= every {
            last_report = Instant::now();
            report_progress(
                stores,
                user_id,
                upload_id,
                ProcessingProgress {
                    bytes_read: bytes_total,
                    bytes_total,
                    conversations_written: written_count.load(Ordering::Relaxed),
                    conversations_total: total,
                },
            )
            .await;
        }
    }
    let times = *times.lock().expect("the times lock is never poisoned");
    note("ms_files", ms(times.files));
    note("ms_rows", ms(times.rows));
    note("ms_sessions", ms(times.sessions));
    note("ms_record", ms(times.record));
    MergeReport::of(&all).note();

    record_totals(stores, user_id).await?;
    let conversation_ids: Vec<ConversationId> = all
        .iter()
        .flat_map(|w| w.conversation_ids.iter().copied())
        .collect();
    stores
        .upload_outcome_store
        .record_outcome(
            user_id,
            upload_id,
            UploadOutcome::Ready { conversation_ids },
        )
        .await?;
    stores.object_store.delete(&key).await?;
    Ok(())
}

/// Counts the user's totals afresh from the records and sessions (two
/// queries, the sessions carrying their message counts) and stores them,
/// raising the data version (§5c, §8b). Counting afresh, rather than adding
/// this upload's changes, means a retried upload can't count twice or not
/// at all.
async fn record_totals(stores: &ProcessingStores, user_id: &UserId) -> Result<(), ProcessingError> {
    let conversations = stores
        .conversation_summary_store
        .list_for_user(user_id)
        .await?
        .len();
    let sessions = stores.session_store.list_sessions(user_id).await?;
    let totals = Totals {
        conversations,
        sessions: sessions.len(),
        your_messages: sessions.iter().map(|s| s.counts.messages).sum(),
        messages: sessions.iter().map(|s| s.message_count).sum(),
    };
    note("totals", serde_json::to_value(totals).unwrap_or_default());
    stores.user_records.record_totals(user_id, totals).await?;
    Ok(())
}

/// What every conversation's write shares.
#[derive(Clone, Copy)]
struct Each<'a> {
    stores: &'a ProcessingStores,
    user_id: &'a UserId,
    upload_id: UploadId,
    facts: &'a timeline_core::conversation_metadata::UploadFacts,
    total: usize,
    written_count: &'a AtomicUsize,
    times: &'a Mutex<StepTimes>,
}

/// Writes the conversation numbered `index` (from 0) of the upload.
async fn write_one(
    each: Each<'_>,
    index: usize,
    conversation: &Kept,
) -> Result<Written, ProcessingError> {
    let written = write_conversation(
        each.stores,
        each.user_id,
        each.upload_id,
        each.facts,
        conversation,
    )
    .await
    .map_err(|source| ProcessingError::SavingConversation {
        number: index + 1,
        total: each.total,
        conversation_id: conversation.main.conversation_id,
        source,
    })?;
    each.written_count.fetch_add(1, Ordering::Relaxed);
    each.times
        .lock()
        .expect("the times lock is never poisoned")
        .add(written.times);
    Ok(written)
}

/// How many of your messages the file carried a review for.
fn reviews_in(kept: &[Kept]) -> usize {
    kept.iter()
        .flat_map(|k| std::iter::once(&k.main).chain(k.branches.iter().map(|b| &b.conversation)))
        .flat_map(|c| &c.entries)
        .filter_map(Entry::your_flags)
        .filter(|f| f.user.is_review())
        .count()
}

/// Refuses the upload if any message is too large for one row (§8b).
fn refuse_oversized(kept: &[Kept]) -> Result<(), ProcessingError> {
    let conversations = kept
        .iter()
        .flat_map(|k| std::iter::once(&k.main).chain(k.branches.iter().map(|b| &b.conversation)));
    for conversation in conversations {
        // Notes hold a few numbers, never anything large.
        for message in conversation.entries.iter().filter_map(Entry::as_message) {
            // Unreachable backstop: stored entries are plain data, which
            // always serializes.
            let bytes = serde_json::to_vec(message)
                .expect("a message serializes")
                .len();
            if bytes > LARGEST_ROW {
                return Err(ProcessingError::MessageTooLarge {
                    conversation_id: conversation.conversation_id,
                    message_id: message.key.id,
                    bytes,
                });
            }
        }
    }
    Ok(())
}

/// What processing a file did to conversations already stored (plan
/// `2026-10-05-screen-flow.md` §8b-2), for the run's log line.
#[derive(Default)]
struct MergeReport {
    new: usize,
    already_present: usize,
    gained_messages: usize,
    earliest_added: Option<chrono::DateTime<chrono::Utc>>,
    latest_added: Option<chrono::DateTime<chrono::Utc>>,
    fewer_than_file: usize,
    untimed_skipped: usize,
}

impl MergeReport {
    fn of(all: &[Written]) -> Self {
        let mut report = MergeReport::default();
        for w in all {
            if !w.already_present {
                report.new += 1;
                continue;
            }
            report.already_present += 1;
            if let Some((start, end)) = w.gained {
                report.gained_messages += 1;
                report.earliest_added = Some(report.earliest_added.map_or(start, |e| e.min(start)));
                report.latest_added = Some(report.latest_added.map_or(end, |l| l.max(end)));
            }
            if w.fewer_than_file {
                report.fewer_than_file += 1;
            }
            report.untimed_skipped += w.untimed_skipped;
        }
        report
    }

    /// Logged only when the file met conversations already stored; for a
    /// file of only new conversations every count would just repeat
    /// `conversations`.
    fn note(&self) {
        if self.already_present == 0 {
            return;
        }
        note("conversations_new", self.new);
        note("conversations_already_present", self.already_present);
        note("conversations_gained_messages", self.gained_messages);
        if let (Some(earliest), Some(latest)) = (self.earliest_added, self.latest_added) {
            note("earliest_added_message", earliest.to_rfc3339());
            note("latest_added_message", latest.to_rfc3339());
        }
        if self.fewer_than_file > 0 {
            note("conversations_fewer_than_file", self.fewer_than_file);
        }
        if self.untimed_skipped > 0 {
            note("messages_of_unknown_time_skipped", self.untimed_skipped);
        }
    }
}
