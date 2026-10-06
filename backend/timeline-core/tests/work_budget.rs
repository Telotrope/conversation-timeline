//! The per-request work limit of plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8c.

use std::num::NonZeroUsize;
use std::time::Duration;

use timeline_core::work_budget::{BudgetSetting, REQUEST_WORK_LIMIT};

#[test]
fn a_step_budget_allows_exactly_its_steps() {
    let mut budget = BudgetSetting::Steps(NonZeroUsize::new(3).unwrap()).start();
    assert!(budget.take_step());
    assert!(budget.take_step());
    assert!(budget.take_step());
    assert!(!budget.take_step());
    assert!(!budget.take_step());
}

/// A request always makes progress: even with no time left, its first step
/// is allowed.
#[test]
fn a_clock_budget_allows_the_first_step_and_none_once_its_time_is_up() {
    let mut budget = BudgetSetting::Clock(Duration::ZERO).start();
    assert!(budget.take_step());
    assert!(!budget.take_step());
}

#[test]
fn a_clock_budget_allows_steps_until_its_time_is_up() {
    let mut budget = BudgetSetting::Clock(Duration::from_millis(30)).start();
    assert!(budget.take_step());
    assert!(budget.take_step());
    std::thread::sleep(Duration::from_millis(40));
    assert!(!budget.take_step());
}

#[test]
fn the_server_starts_no_step_after_9_seconds() {
    assert_eq!(REQUEST_WORK_LIMIT, Duration::from_secs(9));
}

/// A flag save's recount is never split (§8c): its budget doesn't run out.
#[test]
fn an_unlimited_budget_allows_every_step() {
    let mut budget = timeline_core::work_budget::WorkBudget::unlimited();
    assert!((0..100_000).all(|_| budget.take_step()));
}
