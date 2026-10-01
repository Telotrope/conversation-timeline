//! The upload-processing Lambda: runs when a raw upload lands in S3 (the
//! template's `ProcessUploadFunction`). Wiring only, like `main.rs`; the
//! handler is `timeline_api::s3_trigger::handle_s3_event`, tested in
//! `tests/s3_trigger.rs`. See the migration plan's §V2e, E2.
//!
//! A missing setting stops start-up with a message naming it; there is no
//! local fallback.

use aws_lambda_events::event::s3::S3Event;
use lambda_runtime::{service_fn, LambdaEvent};
use timeline_api::aws_settings::StorageSettings;
use timeline_api::aws_state::{build_processing_stores, AwsClients};
use timeline_api::s3_trigger::handle_s3_event;

#[tokio::main]
async fn main() -> Result<(), lambda_runtime::Error> {
    let settings = StorageSettings::from_lookup(|name| std::env::var(name).ok())
        .unwrap_or_else(|e| panic!("cannot start: {e}"));
    let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
    let clients = AwsClients {
        s3: aws_sdk_s3::Client::new(&sdk_config),
        dynamodb: aws_sdk_dynamodb::Client::new(&sdk_config),
    };
    let stores = build_processing_stores(&settings, clients);
    let stores = &stores;
    lambda_runtime::run(service_fn(move |event: LambdaEvent<S3Event>| async move {
        // A failure makes Lambda retry the event; see `s3_trigger`'s doc.
        handle_s3_event(event.payload, stores)
            .await
            .map_err(lambda_runtime::Error::from)
    }))
    .await
}
