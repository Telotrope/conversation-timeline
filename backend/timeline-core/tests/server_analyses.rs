//! The two server analyses' numbers (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5c). The
//! expected buckets, hours and weekdays were produced by the page's own
//! `computeTrendAnalysis` bucket rule and `Date` methods, run in Node with
//! `TZ` set to each zone on 2026-10-06, so the port is checked against the
//! code it replaces, quirks included: a week number can be one lower just
//! after the spring clock change, because the rule divides elapsed
//! milliseconds by 24 hours.
//!
//! These tests replace `analyses.test.js`'s L55 ("the trend buckets by week
//! by default, or by month"), L70 ("time of day counts by hour and
//! weekday") and the trend and time-of-day parts of L111 (§10b).

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use timeline_core::flag_values::{FlagOverrides, FlagSet, MessageFlags};
use timeline_core::flag_view::FlagView;
use timeline_core::server_analyses::{
    AnalysisNumbers, AnalysisRequest, Bucket, ServerAnalysis, TrendGranularity,
};
use timeline_core::MessageTime;

fn at(text: &str) -> MessageTime {
    MessageTime::Known(
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc),
    )
}

fn flagged() -> MessageFlags {
    MessageFlags {
        auto: Some(FlagSet {
            caps: true,
            critical: false,
            angry: false,
        }),
        user: FlagOverrides::default(),
    }
}

fn request(analysis: ServerAnalysis, zone: Tz) -> AnalysisRequest {
    AnalysisRequest {
        analysis,
        view: FlagView::Both,
        zone,
    }
}

/// The one bucket `time` lands in.
fn bucket_of(granularity: TrendGranularity, zone: Tz, time: &str) -> String {
    let request = request(ServerAnalysis::Trend(granularity), zone);
    let mut numbers = request.empty();
    request.count(&mut numbers, at(time), &flagged());
    let AnalysisNumbers::Trend { buckets, .. } = numbers else {
        panic!("a trend request makes trend numbers");
    };
    assert_eq!(buckets.len(), 1);
    buckets.into_keys().next().unwrap()
}

/// (instant, week in New York, month in New York, week in UTC, week in
/// Kolkata, month in Kolkata), from the page's code in Node.
const SAMPLES: &[(&str, &str, &str, &str, &str, &str)] = &[
    (
        "2026-01-01T04:59:00Z",
        "2025-W52",
        "2025-12",
        "2026-W00",
        "2026-W00",
        "2026-01",
    ),
    (
        "2026-01-01T05:00:00Z",
        "2026-W00",
        "2026-01",
        "2026-W00",
        "2026-W00",
        "2026-01",
    ),
    (
        "2026-01-03T12:00:00Z",
        "2026-W00",
        "2026-01",
        "2026-W00",
        "2026-W00",
        "2026-01",
    ),
    (
        "2026-01-04T05:30:00Z",
        "2026-W01",
        "2026-01",
        "2026-W01",
        "2026-W01",
        "2026-01",
    ),
    (
        "2026-03-08T07:30:00Z",
        "2026-W10",
        "2026-03",
        "2026-W10",
        "2026-W10",
        "2026-03",
    ),
    // Just after New York's spring clock change: the page says week 10.
    (
        "2026-03-15T04:30:00Z",
        "2026-W10",
        "2026-03",
        "2026-W11",
        "2026-W11",
        "2026-03",
    ),
    (
        "2026-03-15T03:30:00Z",
        "2026-W10",
        "2026-03",
        "2026-W11",
        "2026-W11",
        "2026-03",
    ),
    (
        "2026-11-01T05:30:00Z",
        "2026-W44",
        "2026-11",
        "2026-W44",
        "2026-W44",
        "2026-11",
    ),
    (
        "2026-12-31T23:59:00Z",
        "2026-W52",
        "2026-12",
        "2026-W52",
        "2027-W00",
        "2027-01",
    ),
    (
        "2027-01-01T00:00:00Z",
        "2026-W52",
        "2026-12",
        "2027-W00",
        "2027-W00",
        "2027-01",
    ),
    (
        "2024-12-29T12:00:00Z",
        "2024-W52",
        "2024-12",
        "2024-W52",
        "2024-W52",
        "2024-12",
    ),
];

#[test]
fn weeks_and_months_match_the_pages_rule_in_each_zone() {
    for (time, ny_week, ny_month, utc_week, kolkata_week, kolkata_month) in SAMPLES {
        assert_eq!(
            bucket_of(TrendGranularity::Week, chrono_tz::America::New_York, time),
            *ny_week,
            "{time}"
        );
        assert_eq!(
            bucket_of(TrendGranularity::Month, chrono_tz::America::New_York, time),
            *ny_month,
            "{time}"
        );
        assert_eq!(
            bucket_of(TrendGranularity::Week, chrono_tz::UTC, time),
            *utc_week,
            "{time}"
        );
        assert_eq!(
            bucket_of(TrendGranularity::Week, chrono_tz::Asia::Kolkata, time),
            *kolkata_week,
            "{time}"
        );
        assert_eq!(
            bucket_of(TrendGranularity::Month, chrono_tz::Asia::Kolkata, time),
            *kolkata_month,
            "{time}"
        );
    }
}

/// (instant, hour and weekday from Sunday in New York, in Kolkata), from the
/// page's `getHours` and `getDay` in Node.
const HOURS: &[(&str, usize, usize, usize, usize)] = &[
    ("2026-01-01T04:59:00Z", 23, 3, 10, 4),
    ("2026-03-08T07:30:00Z", 3, 0, 13, 0),
    ("2026-03-15T03:30:00Z", 23, 6, 9, 0),
    ("2026-12-31T23:59:00Z", 18, 4, 5, 5),
];

#[test]
fn hours_and_weekdays_are_local_to_the_zone() {
    for (time, ny_hour, ny_dow, kolkata_hour, kolkata_dow) in HOURS {
        for (zone, hour, dow) in [
            (chrono_tz::America::New_York, ny_hour, ny_dow),
            (chrono_tz::Asia::Kolkata, kolkata_hour, kolkata_dow),
        ] {
            let request = request(ServerAnalysis::TimeOfDay, zone);
            let mut numbers = request.empty();
            request.count(&mut numbers, at(time), &flagged());
            let AnalysisNumbers::TimeOfDay { by_hour, by_dow } = numbers else {
                panic!("a time-of-day request makes time-of-day numbers");
            };
            let one = Bucket {
                total: 1,
                flagged: 1,
            };
            assert_eq!(by_hour[*hour], one, "{time} in {zone}");
            assert_eq!(by_hour.iter().filter(|b| b.total > 0).count(), 1);
            assert_eq!(by_dow[*dow], one, "{time} in {zone}");
            assert_eq!(by_dow.iter().filter(|b| b.total > 0).count(), 1);
        }
    }
}

#[test]
fn empty_numbers_have_24_hours_7_days_and_no_buckets() {
    let time_of_day = request(ServerAnalysis::TimeOfDay, chrono_tz::UTC).empty();
    assert_eq!(
        time_of_day,
        AnalysisNumbers::TimeOfDay {
            by_hour: vec![Bucket::default(); 24],
            by_dow: vec![Bucket::default(); 7],
        }
    );
    let trend = request(
        ServerAnalysis::Trend(TrendGranularity::Month),
        chrono_tz::UTC,
    )
    .empty();
    assert_eq!(
        trend,
        AnalysisNumbers::Trend {
            granularity: TrendGranularity::Month,
            buckets: Default::default(),
        }
    );
}

/// Replaces the trend and time-of-day parts of `analyses.test.js` L111:
/// with only your flags shown, only reviewed messages count; a flagged
/// message counts as flagged; a message of unknown time is never counted.
#[test]
fn only_messages_that_count_toward_rates_are_counted() {
    let mut yours_only = request(
        ServerAnalysis::Trend(TrendGranularity::Week),
        chrono_tz::UTC,
    );
    yours_only.view = FlagView::Yours;
    let mut numbers = yours_only.empty();
    let unreviewed = flagged();
    let reviewed_clean = MessageFlags {
        auto: Some(FlagSet {
            caps: true,
            critical: true,
            angry: true,
        }),
        user: FlagOverrides {
            caps: Some(false),
            critical: None,
            angry: None,
        },
    };
    let reviewed_flagged = MessageFlags {
        auto: None,
        user: FlagOverrides {
            caps: None,
            critical: Some(true),
            angry: None,
        },
    };
    let time = at("2026-01-05T12:00:00Z");
    yours_only.count(&mut numbers, time, &unreviewed);
    yours_only.count(&mut numbers, time, &reviewed_clean);
    yours_only.count(&mut numbers, time, &reviewed_flagged);
    yours_only.count(&mut numbers, MessageTime::Unknown, &reviewed_flagged);
    let AnalysisNumbers::Trend { buckets, .. } = numbers else {
        panic!("trend numbers");
    };
    assert_eq!(
        buckets.into_iter().collect::<Vec<_>>(),
        vec![(
            "2026-W01".to_string(),
            Bucket {
                total: 2,
                flagged: 1
            }
        )]
    );

    let mut neither = request(ServerAnalysis::TimeOfDay, chrono_tz::UTC);
    neither.view = FlagView::Neither;
    let mut numbers = neither.empty();
    neither.count(&mut numbers, time, &reviewed_flagged);
    assert_eq!(numbers, neither.empty());
}

#[test]
fn each_option_gives_its_own_key() {
    let key = |analysis, view, zone| {
        AnalysisRequest {
            analysis,
            view,
            zone,
        }
        .key()
        .to_string()
    };
    let ny = chrono_tz::America::New_York;
    assert_eq!(
        key(
            ServerAnalysis::Trend(TrendGranularity::Week),
            FlagView::Both,
            ny
        ),
        "trend#week#both#America/New_York"
    );
    assert_eq!(
        key(
            ServerAnalysis::Trend(TrendGranularity::Month),
            FlagView::Automatic,
            ny
        ),
        "trend#month#automatic#America/New_York"
    );
    assert_eq!(
        key(ServerAnalysis::TimeOfDay, FlagView::Yours, chrono_tz::UTC),
        "time_of_day#yours#UTC"
    );
    let neither = AnalysisRequest {
        analysis: ServerAnalysis::TimeOfDay,
        view: FlagView::Neither,
        zone: chrono_tz::UTC,
    }
    .key();
    assert_eq!(neither.as_str(), "time_of_day#neither#UTC");
}

/// Guinea-Bissau's clocks went from 23:59 on 31 December 1974 straight to
/// 01:00 (found by searching the time-zone database). The page's
/// `new Date(1975, 0, 1)` then lands on 01:00, and so does the port; the
/// expected week is the page's, from Node.
#[test]
fn a_year_whose_first_midnight_did_not_exist_counts_from_its_first_hour() {
    let week = bucket_of(
        TrendGranularity::Week,
        chrono_tz::Africa::Bissau,
        "1975-01-05T12:00:00Z",
    );
    assert_eq!(week, "1975-W01");
}
