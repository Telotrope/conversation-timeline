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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_loaded_a_realistic_number_of_words() {
        // Sanity check on the data extraction, not a load-bearing exact
        // count — the decisions doc describes it as "~64,000 words".
        let count = DICTIONARY.len();
        assert!(count > 60_000, "got {count} words");
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
}
