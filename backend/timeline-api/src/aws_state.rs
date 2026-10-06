//! The Lambda's `AppState`: the real S3 and DynamoDB adapters and real
//! Cognito login checks. See the migration plan's §V2d.
//!
//! Until §V2d the Lambda branch of `main.rs` used the same in-memory stores
//! and throwaway dev key pair as local development, so a deployed Lambda
//! would have kept data in one instance's memory and refused real logins.
//! This module deliberately doesn't use `crate::dev_only` at all, so the dev
//! keys can't reach the Lambda through it.

use std::sync::Arc;

use jsonwebtoken::jwk::JwkSet;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};
use timeline_storage::dynamo::conversations_table::DynamoConversationsTable;
use timeline_storage::dynamo::message_rows::DynamoMessageStore;
use timeline_storage::dynamo::sessions_table::DynamoSessionStore;
use timeline_storage::dynamo::user_record_rows::DynamoUserRecordStore;
use timeline_storage::s3::S3ObjectStore;

use crate::aws_settings::{AwsSettings, StorageSettings};
use crate::flag_handles::FlagHandleKey;
use crate::s3_trigger::ProcessingStores;
use crate::state::AppState;

/// The AWS clients the stores use. Passed in rather than created here, so
/// tests can point them at local stand-ins while production creates them
/// from `aws-config`.
pub struct AwsClients {
    pub s3: aws_sdk_s3::Client,
    pub dynamodb: aws_sdk_dynamodb::Client,
}

pub fn build_aws_state(
    settings: &AwsSettings,
    clients: AwsClients,
    jwks: JwkSet,
    flag_handle_key: FlagHandleKey,
) -> AppState {
    let dynamodb = clients.dynamodb.clone();
    let table = settings.conversations_table.as_str();
    let stores = build_processing_stores(&settings.storage(), clients);
    let messages = Arc::new(DynamoMessageStore::new(dynamodb.clone(), table));
    AppState {
        object_store: stores.object_store,
        conversation_summary_store: stores.conversation_summary_store,
        message_reader: messages.clone(),
        user_flag_writer: messages.clone(),
        auto_flag_writer: messages,
        session_store: stores.session_store,
        user_records: stores.user_records,
        analysis_store: Arc::new(DynamoUserRecordStore::new(dynamodb, table)),
        upload_outcome_store: stores.upload_outcome_store,
        verifier: Arc::new(CognitoVerifier::new(
            jwks,
            settings.issuer(),
            settings.client_id.as_str(),
        )),
        flag_handle_key: Arc::new(flag_handle_key),
        budget: BudgetSetting::Clock(REQUEST_WORK_LIMIT),
    }
}

/// The upload-processing Lambda's stores: the same S3 and DynamoDB adapters
/// as the API Lambda, without any login checks (migration plan §V2e, E2).
/// Everything lives in the conversations table (plan
/// `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3).
pub fn build_processing_stores(
    settings: &StorageSettings,
    clients: AwsClients,
) -> ProcessingStores {
    let table = settings.conversations_table.as_str();
    let conversations = Arc::new(DynamoConversationsTable::new(
        clients.dynamodb.clone(),
        table,
    ));
    let messages = Arc::new(DynamoMessageStore::new(clients.dynamodb.clone(), table));
    ProcessingStores {
        object_store: Arc::new(S3ObjectStore::new(
            clients.s3,
            settings.uploads_bucket.as_str(),
        )),
        upload_outcome_store: conversations.clone(),
        conversation_summary_store: conversations,
        message_reader: messages.clone(),
        message_writer: messages,
        session_store: Arc::new(DynamoSessionStore::new(clients.dynamodb.clone(), table)),
        user_records: Arc::new(DynamoUserRecordStore::new(clients.dynamodb, table)),
    }
}

#[derive(Debug)]
pub struct JwksError {
    pub url: String,
    pub reason: String,
}

impl std::fmt::Display for JwksError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not load Cognito's signing keys from {}: {}",
            self.url, self.reason
        )
    }
}

impl std::error::Error for JwksError {}

/// Downloads a key set (Cognito's, in production: `AwsSettings::jwks_url`).
/// The Lambda refuses to start if this fails; it never falls back to the dev
/// keys.
pub async fn fetch_jwks(url: &str) -> Result<JwkSet, JwksError> {
    let error = |reason: String| JwksError {
        url: url.to_string(),
        reason,
    };
    let response = reqwest::get(url)
        .await
        .map_err(|e| error(format!("request failed: {e}")))?;
    if !response.status().is_success() {
        return Err(error(format!("server answered {}", response.status())));
    }
    response
        .json::<JwkSet>()
        .await
        .map_err(|e| error(format!("not a key set: {e}")))
}
