//! Black-box tests for the four-visibility-state auto/user flag matrix,
//! calling only the crate's public API.

use timeline_core::flags::matrix::{effective_flag, is_overridden};

/// Every row of the four-state table from
/// timeline-project-decisions.md §5.3, crossed with both an unset and a set
/// user override, and both possible auto values.
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
    // setting a user value for one flag type must not affect another flag
    // type's own (still-unset) user value.
    let critical_user: Option<bool> = Some(true);
    let angry_user: Option<bool> = None;
    assert!(effective_flag(false, critical_user, true, true));
    assert!(!effective_flag(false, angry_user, true, true));
}
