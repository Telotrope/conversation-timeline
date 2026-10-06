//! How much work one request may do before it answers (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §8b, §8c).
//!
//! Every loop that can run long asks its budget "may I do another step?"
//! before each step, the smallest piece of work there is (one entry read,
//! matched, scanned or written). The real budget answers by the clock; the
//! tests' answers by counting steps, so a test can stop a request after
//! exactly one, two or N steps whatever the computer's speed. The loop's code
//! is the same either way.

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

/// The server's work limit per request: 9 seconds, so that with the round
/// trip the page's bar moves within 10 (§8c).
pub const REQUEST_WORK_LIMIT: Duration = Duration::from_secs(9);

/// Which kind of budget each request gets. Chosen once, at start-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetSetting {
    /// Stop starting steps after this long.
    Clock(Duration),
    /// Allow this many steps per request: tests only.
    Steps(NonZeroUsize),
}

impl BudgetSetting {
    /// A fresh budget for one request, starting now.
    pub fn start(self) -> WorkBudget {
        match self {
            BudgetSetting::Clock(limit) => WorkBudget::Clock {
                deadline: Instant::now() + limit,
                first: true,
            },
            BudgetSetting::Steps(n) => WorkBudget::Steps { left: n.get() },
        }
    }
}

/// One request's budget.
#[derive(Debug, Clone)]
pub enum WorkBudget {
    /// `first` makes sure every request does at least one step, so a
    /// request can never answer without making progress.
    Clock {
        deadline: Instant,
        first: bool,
    },
    Steps {
        left: usize,
    },
}

impl WorkBudget {
    /// Whether another step may be done; if so, the step is counted.
    pub fn take_step(&mut self) -> bool {
        match self {
            WorkBudget::Clock { deadline, first } => {
                let allowed = *first || Instant::now() < *deadline;
                *first = false;
                allowed
            }
            WorkBudget::Steps { left } => {
                if *left == 0 {
                    return false;
                }
                *left -= 1;
                true
            }
        }
    }
}
