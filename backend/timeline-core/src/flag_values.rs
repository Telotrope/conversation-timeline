//! The flag values one message of yours carries: the automatic ones the scan
//! found and your own, kept apart so that the scan can never overwrite yours
//! (migration plan §4.1). Both live on the message's stored row (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §3); before
//! that plan they lived in the `MessageFlags` table, whose port defined
//! these types.

use serde::{Deserialize, Serialize};

/// The three flag types this tool has ever had -- see
/// [timeline-project-decisions.md section 5](../../../timeline-project-decisions.md#L216)
/// for why these three, and why caps is mechanical while the other two are
/// heuristic judgment calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlagSet {
    pub caps: bool,
    pub critical: bool,
    pub angry: bool,
}

/// A partial update to a message's user-owned flag overrides -- `None`
/// means "leave this flag's override untouched," not "clear it." Matches
/// the original UI's "click one checkbox" (one `Some`) and "click Approve"
/// (all three `Some`, computed client-side from the current effective
/// values) cases from
/// [timeline-project-decisions.md section 5.3](../../../timeline-project-decisions.md#L253).
/// Stored as is, it is also your review of a message: `None` for a flag you
/// never set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FlagOverrides {
    pub caps: Option<bool>,
    pub critical: Option<bool>,
    pub angry: Option<bool>,
}

impl FlagOverrides {
    /// These overrides laid over `self`: each flag `later` names replaces
    /// this one's, and the rest are kept.
    pub fn updated_by(self, later: FlagOverrides) -> FlagOverrides {
        FlagOverrides {
            caps: later.caps.or(self.caps),
            critical: later.critical.or(self.critical),
            angry: later.angry.or(self.angry),
        }
    }

    /// Whether you have reviewed the message: a value of yours for any flag.
    /// The page calls the same set "reviewed" and "overridden" (`isReviewed`
    /// and `isOverridden` in `frontend/core/flags.js`).
    pub fn is_review(&self) -> bool {
        self.caps.is_some() || self.critical.is_some() || self.angry.is_some()
    }
}

/// One of the three flags, for code that treats each the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    Caps,
    Critical,
    Angry,
}

impl FlagKind {
    pub const ALL: [FlagKind; 3] = [FlagKind::Caps, FlagKind::Critical, FlagKind::Angry];
}

/// Everything stored about one message's flags. `auto` is `None` until the
/// scan has looked at the message; an unscanned message counts as having
/// no automatic flags, as the page has always treated it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MessageFlags {
    pub auto: Option<FlagSet>,
    pub user: FlagOverrides,
}

impl MessageFlags {
    pub fn auto_value(&self, kind: FlagKind) -> bool {
        let set = self.auto.unwrap_or_default();
        match kind {
            FlagKind::Caps => set.caps,
            FlagKind::Critical => set.critical,
            FlagKind::Angry => set.angry,
        }
    }

    pub fn user_value(&self, kind: FlagKind) -> Option<bool> {
        match kind {
            FlagKind::Caps => self.user.caps,
            FlagKind::Critical => self.user.critical,
            FlagKind::Angry => self.user.angry,
        }
    }
}
