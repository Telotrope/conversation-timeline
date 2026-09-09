//! Black-box tests for `detect_critical`, calling only the crate's public API.

use timeline_core::flags::criticism::detect_critical;

#[test]
fn flags_obvious_criticism() {
    for text in [
        "that's just wrong",
        "This is WRONG.",
        "you're hallucinating again",
        "that's not right at all",
        "you failed to check the docs",
        "this info is outdated",
        "you assumed the wrong config",
        "that's misleading",
        "not true, and you know it",
        "clearly wrong answer",
        "this seems inaccurate",
        "you got the API wrong",
        "that's incorrect",
        "factually wrong response",
        "you're making this up",
    ] {
        assert!(detect_critical(text), "expected {text:?} to be flagged");
    }
}

#[test]
fn apostrophe_variants_all_match_the_dot_wildcard() {
    // The pattern is "didn.t" — a single wildcard character between "didn"
    // and "t", matching a straight or curly apostrophe (or any other
    // one-character separator).
    for text in ["you didn't check", "you didn\u{2019}t verify"] {
        assert!(detect_critical(text), "expected {text:?} to be flagged");
    }
}

/// "didnt" with *no* separator has zero characters between "didn" and "t",
/// so the single-character wildcard doesn't match — a real, faithfully
/// ported quirk of the original pattern, not a bug.
#[test]
fn no_separator_contraction_does_not_match_the_dot_wildcard() {
    assert!(!detect_critical("you didnt check"));
}

#[test]
fn does_not_flag_unrelated_text() {
    for text in [
        "thanks, that worked perfectly",
        "can you add a test for this",
        "let's ship it",
        "",
    ] {
        assert!(!detect_critical(text), "expected {text:?} to be clean");
    }
}

#[test]
fn is_case_insensitive() {
    assert!(detect_critical("YOU MADE THAT UP"));
    assert!(detect_critical("hAlLuCiNaTiOn"));
}
