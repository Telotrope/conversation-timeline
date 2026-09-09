//! Black-box tests for `detect_angry`, calling only the crate's public API.

use timeline_core::flags::anger::detect_angry;
use timeline_core::vader::polarity_scores;

#[test]
fn lexicon_phrases_always_flag_regardless_of_sentiment_score() {
    for text in [
        "I'm so pissed right now",
        "this is absolutely ridiculous",
        "that is completely unacceptable",
        "I am sick of this",
        "I'm fed up with these errors",
        "wtf is going on here",
        "stop wasting my time",
    ] {
        assert!(detect_angry(text), "expected {text:?} to be flagged");
    }
}

#[test]
fn strongly_negative_sentiment_flags_even_without_a_lexicon_phrase() {
    for text in [
        "This is a disaster. Everything about this is bad, broken, and worthless.",
        "I hate this so much, it is the worst experience I have ever had.",
    ] {
        let compound = polarity_scores(text).compound;
        assert!(
            detect_angry(text),
            "expected {text:?} (compound {compound:.3}) to be flagged"
        );
    }
}

#[test]
fn multiple_exclamation_marks_with_negative_sentiment_flags() {
    assert!(detect_angry("this is bad!!"));
    assert!(detect_angry("everything is broken!!"));
}

#[test]
fn a_single_exclamation_mark_alone_is_not_enough() {
    // Only one "!" — the bangs>=2 branch shouldn't fire, and mild
    // negativity alone shouldn't cross the compound threshold either.
    assert!(!detect_angry("that's a bit disappointing!"));
}

#[test]
fn neutral_and_positive_text_is_not_flagged() {
    for text in [
        "Thanks, this works great!",
        "Can you help me with the config file?",
        "The meeting moved to 3pm.",
        "",
    ] {
        assert!(!detect_angry(text), "expected {text:?} to be clean");
    }
}

#[test]
fn mild_negativity_alone_does_not_cross_the_anger_threshold() {
    // Matches the original design stance: not every negative message is
    // "angry" — only strongly negative ones, or ones matching the explicit
    // lexicon/punctuation signals.
    assert!(!detect_angry("this didn't work as expected"));
}

#[test]
fn is_case_insensitive() {
    assert!(detect_angry("I AM SO PISSED"));
}
