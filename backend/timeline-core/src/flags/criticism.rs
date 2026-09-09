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
