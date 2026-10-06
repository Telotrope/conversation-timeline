//! The two analyses computed on the server from each message's own time
//! (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c): the
//! flag rate over time, by week or month, and by hour of day and day of
//! week. Both are counted in the viewer's time zone, which the page names.
//! They were `computeTrendAnalysis` and `computeTimeOfDayAnalysis` in
//! `frontend/core/analyses.js`; the bucket rules are ported exactly.
//!
//! A message counts only when it [counts toward
//! rates](crate::flag_view::FlagView::counts_toward_rates) under the view,
//! and a message whose time is unknown is never counted: it isn't placed on
//! the timeline (§4e).

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::flag_values::MessageFlags;
use crate::flag_view::FlagView;
use crate::message_time::MessageTime;
use crate::walk_cursor::WalkCursor;

/// The flag rate over time's bucket size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrendGranularity {
    Week,
    Month,
}

/// Which analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerAnalysis {
    Trend(TrendGranularity),
    TimeOfDay,
}

/// One analysis, with every option that changes its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalysisRequest {
    pub analysis: ServerAnalysis,
    pub view: FlagView,
    pub zone: Tz,
}

/// Names a saved analysis: the analysis and every option that changes its
/// numbers. Built only from an [`AnalysisRequest`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnalysisKey(String);

impl AnalysisKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AnalysisKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AnalysisRequest {
    pub fn key(&self) -> AnalysisKey {
        let analysis = match self.analysis {
            ServerAnalysis::Trend(TrendGranularity::Week) => "trend#week",
            ServerAnalysis::Trend(TrendGranularity::Month) => "trend#month",
            ServerAnalysis::TimeOfDay => "time_of_day",
        };
        let view = match self.view {
            FlagView::Automatic => "automatic",
            FlagView::Yours => "yours",
            FlagView::Both => "both",
            FlagView::Neither => "neither",
        };
        AnalysisKey(format!("{analysis}#{view}#{}", self.zone.name()))
    }

    /// Counts one message into `numbers`, which [`AnalysisRequest::empty`]
    /// made for this request.
    pub fn count(&self, numbers: &mut AnalysisNumbers, time: MessageTime, flags: &MessageFlags) {
        let MessageTime::Known(at) = time else {
            return;
        };
        if !self.view.counts_toward_rates(flags) {
            return;
        }
        let flagged = self.view.is_flagged(flags);
        match numbers {
            AnalysisNumbers::Trend {
                granularity,
                buckets,
            } => {
                buckets
                    .entry(bucket_key(*granularity, at, self.zone))
                    .or_default()
                    .add(flagged);
            }
            AnalysisNumbers::TimeOfDay { by_hour, by_dow } => {
                let local = at.with_timezone(&self.zone);
                by_hour[local.hour() as usize].add(flagged);
                by_dow[local.weekday().num_days_from_sunday() as usize].add(flagged);
            }
        }
    }

    /// Numbers with nothing counted yet.
    pub fn empty(&self) -> AnalysisNumbers {
        match self.analysis {
            ServerAnalysis::Trend(granularity) => AnalysisNumbers::Trend {
                granularity,
                buckets: BTreeMap::new(),
            },
            ServerAnalysis::TimeOfDay => AnalysisNumbers::TimeOfDay {
                by_hour: vec![Bucket::default(); 24],
                by_dow: vec![Bucket::default(); 7],
            },
        }
    }
}

/// Counted messages in one bucket, and how many were flagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bucket {
    pub total: usize,
    pub flagged: usize,
}

impl Bucket {
    fn add(&mut self, flagged: bool) {
        self.total += 1;
        if flagged {
            self.flagged += 1;
        }
    }
}

/// An analysis's numbers, as the page draws them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnalysisNumbers {
    /// Keyed `2026-W07` (weeks) or `2026-02` (months), which sort in time
    /// order.
    Trend {
        granularity: TrendGranularity,
        buckets: BTreeMap<String, Bucket>,
    },
    /// 24 hours from midnight; 7 days from Sunday.
    TimeOfDay {
        by_hour: Vec<Bucket>,
        by_dow: Vec<Bucket>,
    },
}

/// Where a saved analysis's walk over the messages stopped, when it hasn't
/// finished (§8c: the two server analyses keep their state between parts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unfinished {
    pub cursor: WalkCursor,
    pub sessions_done: usize,
}

/// A saved analysis: its numbers so far, the data version they were
/// counted from, and where to carry on if unfinished.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedAnalysis {
    pub data_version: u64,
    pub numbers: AnalysisNumbers,
    pub unfinished: Option<Unfinished>,
}

/// The page's bucket rule: months as `YYYY-MM`; weeks as `YYYY-Www`, weeks
/// starting on Sunday and numbered from 1 January, both in local time.
fn bucket_key(granularity: TrendGranularity, at: DateTime<Utc>, zone: Tz) -> String {
    let local = at.with_timezone(&zone);
    let year = local.year();
    match granularity {
        TrendGranularity::Month => format!("{year}-{:02}", local.month()),
        TrendGranularity::Week => {
            let first = local_new_year(zone, year);
            let day_of_year = (at - first.with_timezone(&Utc))
                .num_milliseconds()
                .div_euclid(86_400_000);
            let week =
                (day_of_year + i64::from(first.weekday().num_days_from_sunday())).div_euclid(7);
            format!("{year}-W{week:02}")
        }
    }
}

/// Local midnight on 1 January, as the page's `new Date(year, 0, 1)` makes
/// it. Where a zone skips midnight that day, the page lands on the first
/// time after the gap; the first hour that exists is taken here.
fn local_new_year(zone: Tz, year: i32) -> DateTime<Tz> {
    (0..24)
        .find_map(|hour| zone.with_ymd_and_hms(year, 1, 1, hour, 0, 0).earliest())
        // Unreachable backstop: no zone skips a whole day's hours.
        .expect("some hour of 1 January exists in every zone")
}
