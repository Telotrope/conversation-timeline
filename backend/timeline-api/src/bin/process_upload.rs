//! The upload-processing Lambda: runs when a raw upload lands in S3 (the
//! template's `ProcessUploadFunction`). Wiring only, like `main.rs`; the
//! handler is `timeline_api::s3_trigger::handle_s3_event`, tested in
//! `tests/s3_trigger.rs`. See the migration plan's §V2e, E2.
//!
//! A missing setting stops start-up with a message naming it; there is no
//! local fallback. The notification arrives as plain JSON so it can be
//! logged as AWS sent it when `TIMELINE_LOG_S3_EVENTS` is `on` (plan E9).

use lambda_runtime::{service_fn, LambdaEvent};
use serde_json::Value;
use timeline_api::aws_settings::{DeliberateFailure, EventLogging, StorageSettings};
use timeline_api::aws_state::{build_processing_stores, AwsClients};
use timeline_api::s3_trigger::handle_raw_s3_event_recorded;

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    // Counts each run's AWS calls for its log line (s3_trigger).
    timeline_api::aws_call_counter::install();
    let settings = StorageSettings::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let logging = EventLogging::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let failure = DeliberateFailure::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let clients = AwsClients {
        s3: aws_sdk_s3::Client::new(&sdk_config),
        dynamodb: aws_sdk_dynamodb::Client::new(&sdk_config),
    };
    let stores = build_processing_stores(&settings, clients);
    let stores = &stores;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<Value>| async move {
        // A failure makes Lambda retry the event; see `s3_trigger`'s doc.
        handle_raw_s3_event_recorded(
            event.payload,
            stores,
            logging,
            failure,
            |line| println!("{line}"),
            &|line| println!("{line}"),
        )
        .await
        .map_err(lambda_runtime::Error::from)
    }))
    .await
}
