//! Measures what processing one upload costs, so the processing Lambda's
//! memory and time limits come from a measurement rather than a guess
//! (migration plan §V2e, E2). Run through `scripts/measure-processing.sh`.
//!
//! Runs `process_upload` once on the file named on the command line,
//! against the in-memory stores, and prints the file size, the number of
//! conversations, the time taken and the process's peak memory. Peak memory
//! is read from Linux's `/proc/self/status` (`VmHWM`), so this runs on
//! Linux only.
//!
//! The in-memory object store keeps its own copy of the file and hands back
//! another on read, so the peak here includes roughly one more copy of the
//! file than the Lambda holds (S3 hands back one copy). It overstates, not
//! understates.

use std::time::Instant;

use timeline_api::processing::process_upload;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::uploads::raw_object_key;
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::message_flags::InMemoryMessageFlagsStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;

fn peak_memory_kb() -> String {
    let status = std::fs::read_to_string("/proc/self/status")
        .unwrap_or_else(|e| panic!("reading /proc/self/status: {e}"));
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| panic!("no VmHWM line in /proc/self/status"))
}

#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| panic!("usage: measure_processing <conversations.json>"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    let size = bytes.len();

    let objects = InMemoryObjectStore::new();
    let outcomes = InMemoryUploadOutcomeStore::new();
    let summaries = InMemoryConversationSummaryStore::new();
    let flags = InMemoryMessageFlagsStore::new();
    let user = UserId("measure".to_string());
    let upload = UploadId(uuid::Uuid::new_v4());
    objects
        .put(&raw_object_key(&user, upload), bytes)
        .await
        .unwrap_or_else(|e| panic!("storing the file: {e}"));

    let start = Instant::now();
    process_upload(&objects, &outcomes, &summaries, &flags, &user, upload)
        .await
        .unwrap_or_else(|e| panic!("processing failed: {e}"));
    let elapsed = start.elapsed();

    let conversations = summaries
        .list_for_user(&user)
        .await
        .unwrap_or_else(|e| panic!("listing summaries: {e}"))
        .len();
    println!("file bytes:     {size}");
    println!("conversations:  {conversations}");
    println!("processing:     {:.2} s", elapsed.as_secs_f64());
    println!("peak memory:    {}", peak_memory_kb());
}
