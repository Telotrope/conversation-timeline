//! ALL-CAPS emphasis detection — mechanical, not a judgment call. A caps
//! token only counts if its lowercased form is a genuine English dictionary
//! word, which is what correctly excludes acronyms (IRS, DARPA) and
//! conventionally-capitalized short words (OK, TV) without a hand-maintained
//! exclude list covering every acronym.
//!
//! Port of `findEmphasisCapsWords` at
//! [timeline.html:64834-64841](../../../../timeline.html#L64834), backed by
//! the same dictionary (Debian's `wamerican`/SCOWL word list, permissively
//! licensed for any use including commercial — see
//! [timeline-project-decisions.md:227-233](../../../../timeline-project-decisions.md#L227))
//! extracted verbatim from [timeline.html:956-64830](../../../../timeline.html#L956)
//! into `data/dictionary_words.txt`.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

const DICTIONARY_WORDS_RAW: &str = include_str!("../../data/dictionary_words.txt");

/// Real dictionary words that are, in practice, overwhelmingly used as
/// abbreviations rather than the actual word when written in all caps.
const CAPS_EXCLUDE: [&str; 2] = ["id", "eta"];

static CAPS_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z]{2,}\b").expect("static regex is valid"));

static DICTIONARY: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| DICTIONARY_WORDS_RAW.lines().collect());

/// Every all-caps (2+ letter) token in `text` whose lowercase form is a real
/// dictionary word and isn't in the exclude list, in the order they appear.
pub fn find_emphasis_caps_words(text: &str) -> Vec<String> {
    CAPS_TOKEN_RE
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .filter(|w| {
            let lw = w.to_lowercase();
            DICTIONARY.contains(lw.as_str()) && !CAPS_EXCLUDE.contains(&lw.as_str())
        })
        .collect()
}

/// Whether `text` contains at least one emphasis-caps word.
pub fn has_emphasis_caps(text: &str) -> bool {
    !find_emphasis_caps_words(text).is_empty()
}
