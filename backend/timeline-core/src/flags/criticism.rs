//! Criticism-of-Claude detection: a keyword net cast deliberately wide
//! (favors catching real instances over avoiding false positives), meant to
//! be reviewed and corrected by hand afterward rather than trusted outright.
//!
//! Port of `CRITICISM_RE`/`detectCritical` at
//! [timeline.html:64846-64855](../../../../timeline.html#L64846) and
//! [timeline.html:64885-64887](../../../../timeline.html#L64885).

use std::sync::LazyLock;

use regex::Regex;

const PATTERNS: &[&str] = &[
    r"\bwrong\b",
    r"\bincorrect\b",
    r"\binaccurate\b",
    "not accurate",
    "you didn.t (check|verify|search|look)",
    "didn.t search",
    "didn.t verify",
    "poorly researched",
    "bad research",
    "no research",
    "you made (that|this) up",
    "hallucinat",
    "that.s not right",
    "this is wrong",
    "you failed to",
    "outdated",
    "stale (info|data|information)",
    "you assumed",
    "misleading",
    "not true",
    "false information",
    r"\berror\b",
    r"\bmistake\b",
    "doesn.t hold up",
    "clearly wrong",
    "implausible",
    "seems inaccurate",
    "you got .* wrong",
    "that.s incorrect",
    "factually wrong",
    "you.re making (this|that) up",
];

static CRITICISM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("(?i){}", PATTERNS.join("|"))).expect("static regex is valid")
});

pub fn detect_critical(text: &str) -> bool {
    CRITICISM_RE.is_match(text)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// "didnt" with *no* separator has zero characters between "didn" and
    /// "t", so the single-character wildcard doesn't match — a real,
    /// faithfully-ported quirk of the original pattern, not a bug.
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
}
