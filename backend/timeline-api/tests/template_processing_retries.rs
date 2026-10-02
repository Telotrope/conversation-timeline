//! The template's retry setting for the processing function, and its "on
//! failure" destination (plan `2026-10-02-upload-processing-failures.md`
//! §2, §3). The page says "attempt 2 of 3" from `MAX_PROCESSING_ATTEMPTS`;
//! AWS makes the first attempt plus `MaximumRetryAttempts` retries, so the
//! two must agree.
//!
//! Plain text checks, crude in the way the migration plan's C16 describes.
//! Whether AWS calls the destination is checked only on AWS (phase A's live
//! test).

use timeline_api::s3_trigger::MAX_PROCESSING_ATTEMPTS;

const TEMPLATE: &str = include_str!("../../../infra/template.yaml");

/// The indented block under `  <name>:` (two-space resources), up to the
/// next line at that indentation or less.
fn block(name: &str) -> String {
    let header = format!("\n  {name}:\n");
    let rest = TEMPLATE
        .split(&header)
        .nth(1)
        .unwrap_or_else(|| panic!("template declares {name}"));
    rest.lines()
        .take_while(|l| l.is_empty() || l.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_retry_count_matches_the_attempts_the_page_shows() {
    let retries = format!("MaximumRetryAttempts: {}", MAX_PROCESSING_ATTEMPTS - 1);
    assert!(block("ProcessUploadFunction").contains(&retries), "expected {retries}");
}

#[test]
fn a_failure_after_the_last_retry_goes_to_the_function_that_records_it() {
    let processing = block("ProcessUploadFunction");
    assert!(processing.contains(
        "OnFailure:\n            Type: Lambda\n            Destination: !GetAtt RecordFailedUploadFunction.Arn"
    ));
    assert!(processing.contains("- LambdaInvokePolicy:\n            FunctionName: !Ref RecordFailedUploadFunction"));
}

#[test]
fn the_recording_function_runs_its_own_binary_and_may_only_write_the_conversations_table() {
    let recorder = block("RecordFailedUploadFunction");
    assert!(recorder.contains("CodeUri: ../backend/target/lambda/record_failed_upload/"));
    let policies = recorder.split("Policies:").nth(1).expect("has policies");
    assert_eq!(
        policies.trim(),
        "- DynamoDBWritePolicy:\n            TableName: !Ref ConversationsTable"
    );
}
