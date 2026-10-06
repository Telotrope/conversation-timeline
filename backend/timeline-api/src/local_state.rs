//! The local server's stores: in-memory adapters for every port, built
//! once and shared between [`AppState`] (the real, Cognito-gated API) and
//! [`DevState`] (the `_dev`-only local testing surface), so an upload sent
//! through `_dev/local-storage` shows up in `GET /conversations`.
//!
//! Each store is built once as its concrete type and handed out as
//! whichever trait handles need it. Keeping the concrete `Arc` is what lets
//! the same object also appear in `DevState::resettable`: an
//! `Arc<dyn ObjectStore>` cannot be turned back into an
//! `Arc<dyn Resettable>`, so the coercion has to happen from the concrete
//! value, not after the fact.
//!
//! In a library, not in `main.rs`, so tests build exactly the state the
//! local server runs with.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::Arc;

use timeline_auth::cognito::CognitoVerifier;
use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};
use timeline_storage::memory::conversations::InMemoryConversationSummaryStore;
use timeline_storage::memory::messages::InMemoryMessageStore;
use timeline_storage::memory::object_store::InMemoryObjectStore;
use timeline_storage::memory::sessions::InMemorySessionStore;
use timeline_storage::memory::uploads::InMemoryUploadOutcomeStore;
use timeline_storage::memory::user_records::InMemoryUserRecordStore;

use crate::dev_only::{DEV_KEYPAIR, DEV_ONLY_CLIENT_ID, DEV_ONLY_ISSUER};
use crate::dev_state::DevState;
use crate::flag_handles::FlagHandleKey;
use crate::s3_trigger::ProcessingStores;
use crate::state::AppState;

/// The environment variable the local test server reads its step budget
/// from: a whole number of steps per request (plan §8c). Unset, requests
/// get the 9-second clock, as on AWS.
pub const BUDGET_STEPS_VAR: &str = "TIMELINE_WORK_BUDGET_STEPS";

/// A step budget that isn't a whole number of 1 or more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidBudget(pub String);

impl fmt::Display for InvalidBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{BUDGET_STEPS_VAR} must be a whole number of 1 or more, not {:?}",
            self.0
        )
    }
}

impl std::error::Error for InvalidBudget {}

/// The budget the setting asks for: counted steps, or the clock when unset.
pub fn budget_from(value: Option<&str>) -> Result<BudgetSetting, InvalidBudget> {
    match value {
        None => Ok(BudgetSetting::Clock(REQUEST_WORK_LIMIT)),
        Some(text) => text
            .trim()
            .parse::<NonZeroUsize>()
            .map(BudgetSetting::Steps)
            .map_err(|_| InvalidBudget(text.to_string())),
    }
}

/// Builds the in-memory stores once and exposes them as both states.
pub fn build_local_state(
    flag_handle_key: FlagHandleKey,
    budget: BudgetSetting,
) -> (AppState, DevState) {
    // Forces DEV_KEYPAIR's generation to happen here, up front, rather than
    // lazily on the first login/verification -- so a slow key-generation
    // hiccup shows up at startup, not on some later request.
    let (_, jwks) = &*DEV_KEYPAIR;
    let objects = Arc::new(InMemoryObjectStore::new());
    let conversations = Arc::new(InMemoryConversationSummaryStore::new());
    let messages = Arc::new(InMemoryMessageStore::new());
    let sessions = Arc::new(InMemorySessionStore::new());
    let records = Arc::new(InMemoryUserRecordStore::new());
    let uploads = Arc::new(InMemoryUploadOutcomeStore::new());
    let app_state = AppState {
        object_store: objects.clone(),
        conversation_summary_store: conversations.clone(),
        message_reader: messages.clone(),
        user_flag_writer: messages.clone(),
        auto_flag_writer: messages.clone(),
        session_store: sessions.clone(),
        user_records: records.clone(),
        analysis_store: records.clone(),
        upload_outcome_store: uploads.clone(),
        verifier: Arc::new(CognitoVerifier::new(
            jwks.clone(),
            DEV_ONLY_ISSUER,
            DEV_ONLY_CLIENT_ID,
        )),
        flag_handle_key: Arc::new(flag_handle_key),
        budget,
    };
    let dev_state = DevState {
        processing: ProcessingStores {
            object_store: objects.clone(),
            upload_outcome_store: uploads.clone(),
            conversation_summary_store: conversations.clone(),
            message_reader: messages.clone(),
            message_writer: messages.clone(),
            session_store: sessions.clone(),
            user_records: records.clone(),
        },
        resettable: Arc::new(vec![
            objects,
            conversations,
            messages,
            sessions,
            records,
            uploads,
        ]),
    };
    (app_state, dev_state)
}
