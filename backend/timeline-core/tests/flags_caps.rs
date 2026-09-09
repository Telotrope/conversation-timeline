//! Black-box tests for ALL-CAPS emphasis detection, calling only the crate's
//! public API.

use timeline_core::flags::caps::{find_emphasis_caps_words, has_emphasis_caps};

/// Proxy for "the dictionary is broadly populated with ordinary English
/// vocabulary" — the previous version of this test read the private
/// `DICTIONARY` static's length directly; from outside the crate the only
/// way to observe that is by checking that a wide, unrelated sample of real
/// words is recognized, not by asking for an exact count.
#[test]
fn a_broad_sample_of_ordinary_words_are_all_recognized() {
    for word in [
        "HAPPY", "STRANGE", "BUILDING", "JOURNEY", "ELEPHANT", "COMPUTER", "MOUNTAIN", "WHISPER",
        "GARDEN", "FREEDOM", "KITCHEN", "SILVER", "THUNDER", "VELVET", "HARBOR",
    ] {
        assert!(
            !find_emphasis_caps_words(word).is_empty(),
            "{word} should be recognized as a real word"
        );
    }
}

#[test]
fn common_acronyms_are_never_flagged() {
    for acronym in ["IRS", "DARPA", "ICHRA", "QSEHRA", "OK"] {
        assert!(
            find_emphasis_caps_words(acronym).is_empty(),
            "{acronym} should not be flagged"
        );
    }
}

#[test]
fn real_words_in_caps_are_flagged() {
    for word in ["WRONG", "RIDICULOUS"] {
        assert!(
            !find_emphasis_caps_words(word).is_empty(),
            "{word} should be flagged"
        );
    }
}

#[test]
fn manual_exclude_list_overrides_real_dictionary_words() {
    // "id" and "eta" are real dictionary words but overwhelmingly used as
    // abbreviations, so they're excluded despite passing the dictionary
    // check.
    assert!(find_emphasis_caps_words("ID").is_empty());
    assert!(find_emphasis_caps_words("ETA").is_empty());
}

#[test]
fn single_letter_tokens_never_match_the_two_plus_rule() {
    assert!(find_emphasis_caps_words("I A").is_empty());
}

#[test]
fn lowercase_and_mixed_case_words_are_not_matched() {
    assert!(find_emphasis_caps_words("wrong Wrong wRong").is_empty());
}

#[test]
fn multiple_matches_are_all_returned_in_order() {
    let words = find_emphasis_caps_words("this is WRONG and also RIDICULOUS honestly");
    assert_eq!(words, vec!["WRONG".to_string(), "RIDICULOUS".to_string()]);
}

#[test]
fn has_emphasis_caps_matches_find_emphasis_caps_words_emptiness() {
    assert!(has_emphasis_caps("this is WRONG"));
    assert!(!has_emphasis_caps("IRS DARPA"));
}

#[test]
fn empty_text_has_no_matches() {
    assert!(find_emphasis_caps_words("").is_empty());
}
