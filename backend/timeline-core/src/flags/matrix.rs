//! The four-visibility-state matrix that decides which value — the
//! automatic (heuristic or LLM) detection, or the user's own override — is
//! "effective" for a given flag, given the two global `show_auto`/`show_user`
//! toggles. Exact behavior specified in
//! [timeline-project-decisions.md:266-276](../../../../timeline-project-decisions.md#L266).
//!
//! Port of `hasUserValue`/`effectiveFlag`/`isOverridden` at
//! [timeline.html:65188-65211](../../../../timeline.html#L65188), simplified
//! to operate on already-extracted values rather than a message object and a
//! field-name string — the auto value, the optional user override, and the
//! two toggles are all a caller needs to supply.

/// `user_value` is `Some(_)` exactly when the user has explicitly set this
/// flag on this message (`hasUserValue` in the original); `None` means no
/// opinion stated yet.
pub fn effective_flag(
    auto_value: bool,
    user_value: Option<bool>,
    show_auto: bool,
    show_user: bool,
) -> bool {
    match (show_auto, show_user) {
        // Auto-only view: overrides are ignored entirely (not deleted, just not shown).
        (true, false) => auto_value,
        // Your-tags-only view: auto is not used as a fallback; no stated
        // preference just means "nothing", not "whatever auto thinks".
        (false, true) => user_value.unwrap_or(false),
        (false, false) => false,
        // Both on: normal behavior — your override wins if you've stated one.
        (true, true) => user_value.unwrap_or(auto_value),
    }
}

/// "Overridden" in the ON/ON sense (used for the "auto"/"you" source label).
pub fn is_overridden(user_value: Option<bool>) -> bool {
    user_value.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every row of the four-state table from
    /// timeline-project-decisions.md §5.3, crossed with both an unset and a
    /// set user override, and both possible auto values.
    #[test]
    fn four_state_matrix_matches_the_documented_table() {
        // (show_auto, show_user, auto, user) -> expected
        let cases: &[(bool, bool, bool, Option<bool>, bool)] = &[
            // ON/ON: user wins if set, else auto.
            (true, true, true, None, true),
            (true, true, false, None, false),
            (true, true, true, Some(false), false),
            (true, true, false, Some(true), true),
            // ON/OFF: auto only, override completely ignored.
            (true, false, true, None, true),
            (true, false, false, None, false),
            (true, false, true, Some(false), true),
            (true, false, false, Some(true), false),
            // OFF/ON: user value if explicitly set, else nothing (no auto fallback).
            (false, true, true, None, false),
            (false, true, false, None, false),
            (false, true, true, Some(false), false),
            (false, true, false, Some(true), true),
            // OFF/OFF: always false.
            (false, false, true, None, false),
            (false, false, true, Some(true), false),
        ];
        for &(show_auto, show_user, auto, user, expected) in cases {
            assert_eq!(
                effective_flag(auto, user, show_auto, show_user),
                expected,
                "show_auto={show_auto} show_user={show_user} auto={auto} user={user:?}"
            );
        }
    }

    #[test]
    fn is_overridden_reflects_whether_the_user_stated_a_value() {
        assert!(!is_overridden(None));
        assert!(is_overridden(Some(true)));
        assert!(is_overridden(Some(false)));
    }

    #[test]
    fn clicking_one_checkbox_promotes_only_that_flag_not_the_others() {
        // Regression for the exact scenario called out in the decisions doc:
        // setting a user value for one flag type must not affect another
        // flag type's own (still-unset) user value.
        let critical_user: Option<bool> = Some(true);
        let angry_user: Option<bool> = None;
        assert!(effective_flag(false, critical_user, true, true));
        assert!(!effective_flag(false, angry_user, true, true));
    }
}
