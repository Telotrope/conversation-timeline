//! Measures what processing one upload costs, so the processing Lambda's
//! memory and time limits come from a measurement rather than a guess
//! (migration plan §V2e, E2). Run through `scripts/measure-processing.sh`.
//!
//! Runs `process_upload` once on the file named on the command line,
//! against the local server's in-memory stores, and prints the file size,
//! the number of conversations, the time taken, each step's time and counts
//! (the facts the processing run's log line carries; plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7) and the
//! process's peak memory. Peak memory is read from Linux's
//! `/proc/self/status` (`VmHWM`), so this runs on Linux only. A gzip file is
//! decompressed by processing itself, as the page's slimmed uploads are.
//!
//! The in-memory object store keeps its own copy of the file and hands back
//! another on read, so the peak here includes roughly one more copy of the
//! file than the Lambda holds (S3 hands back one copy). It overstates, not
//! understates.

use std::time::Instant;

use timeline_api::flag_handles::FlagHandleKey;
use timeline_api::local_state::build_local_state;
use timeline_api::processing::process_upload;
use timeline_api::request_record::recording;
use timeline_core::conversation_metadata::UploadFacts;
use timeline_core::labels::{FileName, PersonName};
use timeline_core::ports::ids::{UploadId, UserId};
use timeline_core::ports::uploads::raw_object_key;
use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};

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

    let (_, dev) = build_local_state(
        FlagHandleKey::generate(),
        BudgetSetting::Clock(REQUEST_WORK_LIMIT),
    );
    let stores = dev.processing;
    let user = UserId("measure".to_string());
    let upload = UploadId(uuid::Uuid::new_v4());
    stores
        .object_store
        .put(&raw_object_key(&user, upload), bytes)
        .await
        .unwrap_or_else(|e| panic!("storing the file: {e}"));
    stores
        .upload_outcome_store
        .record_received(
            &user,
            upload,
            UploadFacts {
                file_name: FileName::parse("conversations.json").unwrap(),
                uploaded_at: chrono::Utc::now(),
                file_written_at: None,
                human_name: PersonName::parse("Measure").unwrap(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("recording the upload: {e}"));

    let start = Instant::now();
    let (result, record) = recording(process_upload(&stores, &user, upload)).await;
    let elapsed = start.elapsed();
    result.unwrap_or_else(|e| panic!("processing failed: {e}"));

    let conversations = stores
        .conversation_summary_store
        .list_for_user(&user)
        .await
        .unwrap_or_else(|e| panic!("listing summaries: {e}"))
        .len();
    println!("file bytes:     {size}");
    println!("conversations:  {conversations}");
    println!("processing:     {:.2} s", elapsed.as_secs_f64());
    println!("peak memory:    {}", peak_memory_kb());
    println!(
        "facts:          {}",
        serde_json::Value::Object(record.facts)
    );
}
