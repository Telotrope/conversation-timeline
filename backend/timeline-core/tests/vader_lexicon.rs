//! Black-box tests for the VADER static data, calling only the crate's
//! public API.

use timeline_core::vader::lexicon::{BOOSTER_DICT, B_DECR, B_INCR, LEXICON, NEGATE, SPECIAL_CASES};

#[test]
fn lexicon_loaded_the_full_upstream_word_count() {
    // The upstream file has 7520 *lines*, but 14 words appear twice (e.g.
    // "lol", "ok", "d") — a pre-existing data quirk in the original, not
    // something this port introduced. A HashMap collapses those to 7506
    // unique entries, matching what upstream's own `make_lex_dict` (a plain
    // Python dict, same last-write-wins behavior) would produce too.
    assert_eq!(LEXICON.len(), 7506);
}

#[test]
fn spot_check_known_lexicon_values() {
    assert!((LEXICON["great"] - 3.1).abs() < 1e-9);
    assert!(LEXICON["awful"] < 0.0);
}

#[test]
fn negate_and_booster_lists_are_non_empty_and_disjoint_in_purpose() {
    assert!(NEGATE.contains("not"));
    assert!(NEGATE.contains("never"));
    assert_eq!(BOOSTER_DICT["very"], B_INCR);
    assert_eq!(BOOSTER_DICT["slightly"], B_DECR);
}

#[test]
fn special_cases_loaded() {
    assert_eq!(SPECIAL_CASES["the bomb"], 3.0);
    assert_eq!(SPECIAL_CASES.len(), 10);
}
