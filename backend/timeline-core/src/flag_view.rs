//! The page's two show switches (automatic flags, yours) as one value, and
//! the rules that decide what a message counts as under them. These were
//! the page's `effectiveFlag`, `isFlagged` and `countsTowardRates`
//! (`frontend/core/flags.js`); they moved here unchanged (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §5b) so that
//! the session counts, the message filter and the analyses all mean the
//! same thing by "flagged". Which value wins for one flag is the existing
//! [`crate::flags::matrix::effective_flag`].

use serde::{Deserialize, Serialize};

use crate::flag_values::{FlagKind, MessageFlags};
use crate::flags::matrix::effective_flag;

/// Which flags are shown: the two switches' four combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagView {
    /// Automatic flags only; your corrections are ignored, not deleted.
    Automatic,
    /// Your flags only; an automatic flag you never reviewed shows nothing.
    Yours,
    /// Both, with yours winning where you set one.
    Both,
    /// Neither switch on: nothing is flagged and nothing has a rate.
    Neither,
}

impl FlagView {
    /// The three views that show anything, in the order session counts
    /// store them.
    pub const SHOWING: [FlagView; 3] = [FlagView::Automatic, FlagView::Yours, FlagView::Both];

    fn switches(self) -> (bool, bool) {
        match self {
            FlagView::Automatic => (true, false),
            FlagView::Yours => (false, true),
            FlagView::Both => (true, true),
            FlagView::Neither => (false, false),
        }
    }

    /// Whether `kind` is in effect on a message with these flags.
    pub fn shows(self, flags: &MessageFlags, kind: FlagKind) -> bool {
        let (show_auto, show_user) = self.switches();
        effective_flag(
            flags.auto_value(kind),
            flags.user_value(kind),
            show_auto,
            show_user,
        )
    }

    /// Whether any of the three flags is in effect.
    pub fn is_flagged(self, flags: &MessageFlags) -> bool {
        FlagKind::ALL.iter().any(|kind| self.shows(flags, *kind))
    }

    /// Whether the message has flag values under this view, and so belongs
    /// in a rate's denominator. With automatic flags shown every message
    /// has a value; with only yours shown, only reviewed messages do; with
    /// neither, none do. Counting the rest as "not flagged" would report
    /// unreviewed messages as clean.
    pub fn counts_toward_rates(self, flags: &MessageFlags) -> bool {
        match self {
            FlagView::Automatic | FlagView::Both => true,
            FlagView::Yours => flags.user.is_review(),
            FlagView::Neither => false,
        }
    }
}
