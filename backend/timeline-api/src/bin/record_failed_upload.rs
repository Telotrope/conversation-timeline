//! The processing function's "on failure" destination: records `Failed` for
//! an upload once AWS has given up retrying it (the template's
//! `RecordFailedUploadFunction`; plan
//! `docs/plans/2026-10-02-upload-processing-failures.md` §2). Wiring only;
//! the handler is `timeline_api::failed_upload::handle_failed_invocation`,
//! tested in `tests/failed_upload.rs`.
//!
//! Reads the same settings as the processing function and uses only the
//! conversations table. A missing setting stops start-up naming it.

use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::Value;
use timeline_api::aws_settings::StorageSettings;
use timeline_api::failed_upload::handle_failed_invocation;
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    let settings = StorageSettings::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let store = DynamoConversationsTable::new(
        aws_sdk_dynamodb::Client::new(&sdk_config),
        settings.conversations_table.as_str(),
    );
    let store = &store;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        handle_failed_invocation(event.payload, store)
            .await
            .map_err(lambda_runtime::Error::from)
    }))
    .await
}
