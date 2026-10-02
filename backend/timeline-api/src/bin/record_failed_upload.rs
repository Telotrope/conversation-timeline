//! The processing function's "on failure" destination: records `Failed` for
//! an upload once AWS has given up retrying it (the template's
//! `RecordFailedUploadFunction`; plan
//! `docs/plans/2026-10-02-upload-processing-failures.md` §2). Wiring only;
//! the handler is `timeline_api::failed_upload::handle_failed_invocation`,
//! tested in `tests/failed_upload.rs`.
//!
//! Reads only `TIMELINE_CONVERSATIONS_TABLE`, the one table it writes. A
//! missing setting stops start-up naming it.

use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::Value;
use timeline_api::failed_upload::handle_failed_invocation_logged;
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    let table = std::env::var("TIMELINE_CONVERSATIONS_TABLE")
        .unwrap_or_else(|e| panic!("cannot start: TIMELINE_CONVERSATIONS_TABLE: {e}"));
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = DynamoConversationsTable::new(aws_sdk_dynamodb::Client::new(&sdk_config), table);
    let store = &store;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        handle_failed_invocation_logged(event.payload, store, &|line| println!("{line}"))
            .await
            .map_err(lambda_runtime::Error::from)
    }))
    .await
}
