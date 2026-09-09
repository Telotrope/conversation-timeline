//! Anger detection: an anger-specific keyword/phrase list, combined with a
//! sentiment-intensity score and exclamation-mark bursts. Like criticism
//! detection, this is a first-pass suggestion, not a verdict.
//!
//! Port of `ANGER_LEXICON_RE`/`detectAngry` at
//! [timeline.html:64861-64883](../../../../timeline.html#L64861), with the
//! original AFINN-based `afinnScore` replaced by [`crate::vader::polarity_scores`]
//! (see the migration plan's C2: AFINN is ODbL-licensed, not on this
//! project's approved license list; VADER's upstream is MIT).
//!
//! **Recalibration note:** the original threshold (`score <= -1.0`) was
//! tuned against AFINN's `sum(word scores) / sqrt(token count)` scale, which
//! is unbounded. VADER's `compound` score is normalized to `[-1, 1]` by
//! construction, so the same numeric threshold doesn't transfer — reusing
//! `-1.0` would mean "never triggers" (VADER's compound saturates well
//! before -1.0 in practice). [`ANGER_COMPOUND_THRESHOLD`] was picked by
//! testing against the realistic examples in this module's test suite,
//! favoring recall over precision (matching the original design stance) —
//! the original hand-curated baseline dataset referenced in
//! [timeline-project-decisions.md:243-249](../../../../timeline-project-decisions.md#L243)
//! is not available in this repo to recalibrate against directly (see the
//! migration plan's C1/C9).

use std::sync::LazyLock;

use regex::Regex;

use crate::vader::polarity_scores;

const PATTERNS: &[&str] = &[
    r"\bpissed\b",
    r"\bfurious\b",
    r"\bfrustrat",
    r"\bridiculous\b",
    r"\bunacceptable\b",
    r"\binfuriat",
    r"\bsick of\b",
    r"\bdone with\b",
    r"\bfed up\b",
    r"\bhorrifying\b",
    r"\bhorrific\b",
    r"\bawful\b",
    r"\bterrible\b",
    r"\bstupid\b",
    r"\bidiot",
    r"\bdamn\b",
    r"\bhell\b",
    r"\bcrap\b",
    r"\bwtf\b",
    r"\bmoron\b",
    r"\bwaste of time\b",
    r"\bwasting my time\b",
    r"\bwasted (a lot of|so much) time\b",
    r"\btoo stupid\b",
    r"\bmislead\b",
    r"\bhow bad you are\b",
    r"\bplainly false\b",
    r"\bmake no sense\b",
    "doesn.t make sense",
    r"\bbastardiz",
];

static ANGER_LEXICON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("(?i)({})", PATTERNS.join("|"))).expect("static regex is valid")
});

/// Empirically recalibrated for VADER's `[-1, 1]` compound scale — see the
/// module docs.
pub const ANGER_COMPOUND_THRESHOLD: f64 = -0.6;

pub fn detect_angry(text: &str) -> bool {
    let lex = ANGER_LEXICON_RE.is_match(text);
    let compound = polarity_scores(text).compound;
    let bangs = text.matches('!').count();
    lex || compound <= ANGER_COMPOUND_THRESHOLD || (bangs >= 2 && compound < 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // "angry" — only strongly negative ones, or ones matching the
        // explicit lexicon/punctuation signals.
        assert!(!detect_angry("this didn't work as expected"));
    }

    #[test]
    fn is_case_insensitive() {
        assert!(detect_angry("I AM SO PISSED"));
    }
}
