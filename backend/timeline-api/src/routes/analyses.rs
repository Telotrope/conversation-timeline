//! `GET /analyses/{name}`: the two analyses computed on the server from
//! each message's own time (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c, §8c):
//! `trend` (the flag rate over time, by `granularity` week or month) and
//! `time-of-day` (by hour and weekday). The page sends the show switches
//! (`view`) and its time zone's name (`tz`, e.g. `America/New_York`).
//!
//! **Saved, and carried on.** Each analysis with its options has a saved
//! row holding its numbers so far, the data version they were counted
//! from, and while unfinished where to carry on. A request whose row has
//! the current version and is finished gets the numbers at once; one whose
//! row is unfinished carries on from it within the time limit and saves
//! again; a row of an older version is started over. The page asks again
//! while the answer says `working`, so leaving Analytics stops the work,
//! and the next request with the same options carries on.

use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use timeline_core::flag_view::FlagView;
use timeline_core::message_filter::MessageFilter;
use timeline_core::ports::analyses::AnalysisStore;
use timeline_core::ports::messages::MessageReader;
use timeline_core::ports::sessions::SessionStore;
use timeline_core::ports::user_record::UserRecordStore;
use timeline_core::server_analyses::{
    AnalysisNumbers, AnalysisRequest, SavedAnalysis, ServerAnalysis, TrendGranularity, Unfinished,
};
use timeline_core::stored_message::Entry;
use timeline_core::stored_session::StoredSession;
use timeline_core::walk_cursor::WalkCursor;
use timeline_core::work_budget::BudgetSetting;

use crate::auth_extractor::AuthenticatedUser;
use crate::error::ApiError;
use crate::message_query::{find_messages, EntryVisitor, Flow, WalkOrder, WalkStores};
use crate::request_record::note;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisQuery {
    pub granularity: Option<TrendGranularity>,
    pub view: Option<FlagView>,
    /// An IANA time zone name.
    pub tz: String,
}

/// The answer: the numbers once finished, otherwise how far it has got.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AnalysisReply {
    Done {
        numbers: AnalysisNumbers,
        data_version: u64,
    },
    Working {
        sessions_done: usize,
        sessions_total: usize,
        data_version: u64,
    },
}

fn parse(name: &str, query: AnalysisQuery) -> Result<AnalysisRequest, ApiError> {
    let analysis = match name {
        "trend" => ServerAnalysis::Trend(query.granularity.unwrap_or(TrendGranularity::Week)),
        "time-of-day" => ServerAnalysis::TimeOfDay,
        _ => return Err(ApiError::NotFound),
    };
    let zone = query.tz.parse::<chrono_tz::Tz>().map_err(|_| {
        ApiError::BadRequest(format!(
            "tz must be a time zone's name, such as America/New_York, not {:?}",
            query.tz.chars().take(60).collect::<String>()
        ))
    })?;
    Ok(AnalysisRequest {
        analysis,
        view: query.view.unwrap_or(FlagView::Both),
        zone,
    })
}

struct Count<'a> {
    request: &'a AnalysisRequest,
    numbers: AnalysisNumbers,
}

#[async_trait]
impl EntryVisitor for Count<'_> {
    async fn entry(
        &mut self,
        entry: &Entry,
        _session: &StoredSession,
        _before: WalkCursor,
    ) -> Result<Flow, ApiError> {
        if let Entry::Message(message) = entry {
            self.request.count(
                &mut self.numbers,
                message.key.time(),
                &message.flags.unwrap_or_default(),
            );
        }
        Ok(Flow::Continue)
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn analysis(
    AuthenticatedUser(user_id): AuthenticatedUser,
    State(sessions): State<Arc<dyn SessionStore>>,
    State(messages): State<Arc<dyn MessageReader>>,
    State(user_records): State<Arc<dyn UserRecordStore>>,
    State(analyses): State<Arc<dyn AnalysisStore>>,
    State(budget): State<BudgetSetting>,
    Path(name): Path<String>,
    query: Result<Query<AnalysisQuery>, QueryRejection>,
) -> Result<Json<AnalysisReply>, ApiError> {
    let Query(query) = query.map_err(|r| ApiError::BadRequest(r.body_text()))?;
    let request = parse(&name, query)?;
    let key = request.key();
    let version = user_records.get(&user_id).await?.data_version;
    let saved = analyses
        .get(&user_id, &key)
        .await?
        .filter(|s| s.data_version == version);
    note("analysis", key.to_string());
    let (numbers, start) = match saved {
        Some(SavedAnalysis {
            numbers,
            unfinished: None,
            ..
        }) => {
            note("saved", true);
            return Ok(Json(AnalysisReply::Done {
                numbers,
                data_version: version,
            }));
        }
        Some(SavedAnalysis {
            numbers,
            unfinished: Some(unfinished),
            ..
        }) => (numbers, Some(unfinished.cursor)),
        None => (request.empty(), None),
    };
    note("saved", false);
    let mut count = Count {
        request: &request,
        numbers,
    };
    let end = find_messages(
        WalkStores {
            sessions: sessions.as_ref(),
            messages: messages.as_ref(),
        },
        &user_id,
        &MessageFilter::your_messages(request.view),
        &WalkOrder::Key,
        start,
        &mut budget.start(),
        &mut count,
    )
    .await?;
    let saved = SavedAnalysis {
        data_version: version,
        numbers: count.numbers,
        unfinished: end.cursor.map(|cursor| Unfinished {
            cursor,
            sessions_done: end.sessions_done,
        }),
    };
    analyses.put(&user_id, &key, &saved).await?;
    Ok(Json(match saved.unfinished {
        None => AnalysisReply::Done {
            numbers: saved.numbers,
            data_version: version,
        },
        Some(_) => AnalysisReply::Working {
            sessions_done: end.sessions_done,
            sessions_total: end.sessions_total,
            data_version: version,
        },
    }))
}
