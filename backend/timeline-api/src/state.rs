//! The axum app's shared state, and the `FromRef` impls that let each route
//! handler declare only the one narrow capability it needs (e.g.
//! `State<Arc<dyn UserFlagWriter>>`) rather than the whole state -- this is
//! what makes the auto/user flag separation from the migration plan
//! section 4.1 real at the wiring level, not just at the trait-definition
//! level.
//!
//! Flags live on the message rows (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3); routes read
//! them through `MessageReader` and write them through the two narrow
//! writers below. Only processing holds a `MessageRowWriter`, and it is not
//! in this state.
//!
//! **`AutoFlagWriter` is now part of this state, and that is a deliberate
//! change from the earlier design.** It used to be excluded on the grounds
//! that no user-facing handler should ever be able to write automatic flags,
//! because detection only ever ran in the upload-processing path. Detection
//! is now something the user explicitly asks for (`POST /detect`, see
//! `crate::routes::detect`), so a user-facing route legitimately needs it.
//!
//! What that exclusion was actually protecting is unchanged: automatic and
//! user-confirmed flags remain two separate ports writing two separate
//! stored fields, so an automatic pass still cannot overwrite a flag the
//! user confirmed. The guarantee lives in the port split, not in which
//! struct holds a handle. `UserFlagWriter` and `AutoFlagWriter` are still
//! distinct, and no handler takes both.
//!
//! `UploadOutcomeStore` is here for `GET /uploads/{upload_id}`, which tells
//! the page whether processing has finished (migration plan §V2e, E3). On
//! AWS processing runs in a separate Lambda after the file lands in S3, so
//! the page has to ask. Routes only read it; the port's read and write
//! methods share one trait, and splitting it for one route wasn't worth it
//! (a recorded choice, see the plan).

use std::sync::Arc;

use axum::extract::FromRef;
use timeline_auth::cognito::CognitoVerifier;
use timeline_core::ports::analyses::AnalysisStore;
use timeline_core::ports::conversations::ConversationSummaryStore;
use timeline_core::ports::messages::{AutoFlagWriter, MessageReader, UserFlagWriter};
use timeline_core::ports::object_store::ObjectStore;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::uploads::UploadOutcomeStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::work_budget::BudgetSetting;

use crate::flag_handles::FlagHandleKey;

#[derive(Clone)]
pub struct AppState {
    pub object_store: Arc<dyn ObjectStore>,
    pub conversation_summary_store: Arc<dyn ConversationSummaryStore>,
    pub message_reader: Arc<dyn MessageReader>,
    pub user_flag_writer: Arc<dyn UserFlagWriter>,
    pub auto_flag_writer: Arc<dyn AutoFlagWriter>,
    pub session_store: Arc<dyn SessionStore>,
    pub user_records: Arc<dyn UserRecordStore>,
    pub analysis_store: Arc<dyn AnalysisStore>,
    pub upload_outcome_store: Arc<dyn UploadOutcomeStore>,
    pub verifier: Arc<CognitoVerifier>,
    /// Signs and checks flag handles; see `crate::flag_handles`.
    pub flag_handle_key: Arc<FlagHandleKey>,
    /// How much work one request may do before it answers (plan §8c): the
    /// clock in production, counted steps for the local test server.
    pub budget: BudgetSetting,
}

macro_rules! from_state {
    ($field:ident: $ty:ty) => {
        impl FromRef<AppState> for $ty {
            fn from_ref(state: &AppState) -> Self {
                state.$field.clone()
            }
        }
    };
}

from_state!(object_store: Arc<dyn ObjectStore>);
from_state!(conversation_summary_store: Arc<dyn ConversationSummaryStore>);
from_state!(message_reader: Arc<dyn MessageReader>);
from_state!(user_flag_writer: Arc<dyn UserFlagWriter>);
from_state!(auto_flag_writer: Arc<dyn AutoFlagWriter>);
from_state!(session_store: Arc<dyn SessionStore>);
from_state!(user_records: Arc<dyn UserRecordStore>);
from_state!(analysis_store: Arc<dyn AnalysisStore>);
from_state!(upload_outcome_store: Arc<dyn UploadOutcomeStore>);
from_state!(verifier: Arc<CognitoVerifier>);
from_state!(flag_handle_key: Arc<FlagHandleKey>);

impl FromRef<AppState> for BudgetSetting {
    fn from_ref(state: &AppState) -> Self {
        state.budget
    }
}
