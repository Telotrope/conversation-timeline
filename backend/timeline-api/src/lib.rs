//! The deployed backend's axum app: route handlers behind API Gateway, per
//! V2 of the migration plan. `app::build_router` is the single place the
//! whole app is wired together; `main.rs` just decides whether to run it
//! locally or inside Lambda. `s3_trigger` is the upload-processing Lambda's
//! handler, run by `src/bin/process_upload.rs`.

pub mod app;
pub mod auth_extractor;
pub mod aws_call_counter;
pub mod aws_settings;
pub mod aws_state;
pub mod conversation_rebuild;
pub mod dev_only;
pub mod dev_state;
pub mod error;
pub mod failed_upload;
pub mod flag_handles;
pub mod processing;
pub mod request_log;
pub mod request_record;
pub mod routes;
pub mod s3_trigger;
pub mod state;
