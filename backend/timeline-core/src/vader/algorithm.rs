//! Core VADER (Valence Aware Dictionary and sEntiment Reasoner) scoring
//! algorithm: lexicon lookup, negation, booster/dampener words, ALL-CAPS
//! emphasis, the "but" contrastive-conjunction rule, a small set of
//! multi-word special-case phrases, and punctuation emphasis, normalized to
//! a single `compound` score in `[-1, 1]`.
//!
//! Faithful port of `SentimentIntensityAnalyzer` in the original, MIT-licensed
//! `vaderSentiment.py` (C.J. Hutto). Two upstream features are deliberately
//! **not** ported, because chat messages are plain text, not social-media
//! posts, and upstream itself doesn't ship or use them either:
//! - Emoji-to-text substitution (`emoji_utf8_lexicon.txt`) — no emoji
//!   lexicon is embedded.
//! - `SENTIMENT_LADEN_IDIOMS` — upstream's own comment marks this "future
//!   work, not yet implemented"; it's dead code even in the original.
//!
//! See [`crate::vader::lexicon`] for the embedded word lists this module
//! scores against.

use super::lexicon::{BOOSTER_DICT, C_INCR, LEXICON, NEGATE, N_SCALAR, SPECIAL_CASES};
use super::tokenize::{allcap_differential, is_all_upper, words_and_emoticons};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PolarityScores {
    pub neg: f64,
    pub neu: f64,
    pub pos: f64,
    /// Normalized, weighted composite score in `[-1, 1]` — the single
    /// number most callers want.
    pub compound: f64,
}

/// Scores `text` with the full VADER algorithm. See module docs for the two
/// deliberately-unported upstream features.
pub fn polarity_scores(text: &str) -> PolarityScores {
    let words = words_and_emoticons(text);
    let is_cap_diff = allcap_differential(&words);
    let words_lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();

    let mut sentiments: Vec<f64> = Vec::with_capacity(words.len());
    for i in 0..words.len() {
        if BOOSTER_DICT.contains_key(words_lower[i].as_str()) {
            sentiments.push(0.0);
            continue;
        }
        if words_lower[i] == "kind" && i + 1 < words.len() && words_lower[i + 1] == "of" {
            sentiments.push(0.0);
            continue;
        }
        let valence = sentiment_valence(&words, &words_lower, i, is_cap_diff);
        sentiments.push(valence);
    }

    but_check(&words_lower, &mut sentiments);
    score_valence(&sentiments, text)
}

fn sentiment_valence(words: &[String], words_lower: &[String], i: usize, is_cap_diff: bool) -> f64 {
    let item_lower = words_lower[i].as_str();
    let Some(&base_valence) = LEXICON.get(item_lower) else {
        return 0.0;
    };
    let mut valence = base_valence;

    // "no" immediately before another lexicon word acts as pure negation of
    // that word, not as its own (usually mildly negative) lexicon value.
    if item_lower == "no"
        && i != words.len() - 1
        && LEXICON.contains_key(words_lower[i + 1].as_str())
    {
        valence = 0.0;
    }
    // "no" one, two, or (only as "X or/nor no <word>") three words back
    // negates this word, using the *original* lexicon value.
    let no_negates = (i > 0 && words_lower[i - 1] == "no")
        || (i > 1 && words_lower[i - 2] == "no")
        || (i > 2
            && words_lower[i - 3] == "no"
            && (words_lower[i - 1] == "or" || words_lower[i - 1] == "nor"));
    if no_negates {
        valence = base_valence * N_SCALAR;
    }

    if is_all_upper(&words[i]) && is_cap_diff {
        if valence > 0.0 {
            valence += C_INCR;
        } else {
            valence -= C_INCR;
        }
    }

    for start_i in 0..3usize {
        if i > start_i && !LEXICON.contains_key(words_lower[i - (start_i + 1)].as_str()) {
            let mut s = scalar_inc_dec(&words[i - (start_i + 1)], valence, is_cap_diff);
            if start_i == 1 && s != 0.0 {
                s *= 0.95;
            }
            if start_i == 2 && s != 0.0 {
                s *= 0.9;
            }
            valence += s;
            valence = negation_check(valence, words_lower, start_i, i);
            if start_i == 2 {
                valence = special_idioms_check(valence, words_lower, i);
            }
        }
    }

    least_check(valence, words_lower, i)
}

/// Whether the preceding word(s) boost, dampen, negate, or leave `valence`
/// alone — booster/dampener words only, not full negation (see
/// [`negation_check`] for that).
fn scalar_inc_dec(word: &str, valence: f64, is_cap_diff: bool) -> f64 {
    let word_lower = word.to_lowercase();
    let Some(&base) = BOOSTER_DICT.get(word_lower.as_str()) else {
        return 0.0;
    };
    let mut scalar = base;
    if valence < 0.0 {
        scalar = -scalar;
    }
    if is_all_upper(word) && is_cap_diff {
        if valence > 0.0 {
            scalar += C_INCR;
        } else {
            scalar -= C_INCR;
        }
    }
    scalar
}

fn negated_single(word_lower: &str) -> bool {
    NEGATE.contains(word_lower) || word_lower.contains("n't")
}

fn negation_check(valence: f64, words_lower: &[String], start_i: usize, i: usize) -> f64 {
    match start_i {
        0 => {
            if negated_single(&words_lower[i - 1]) {
                valence * N_SCALAR
            } else {
                valence
            }
        }
        1 => {
            if words_lower[i - 2] == "never"
                && (words_lower[i - 1] == "so" || words_lower[i - 1] == "this")
            {
                valence * 1.25
            } else if words_lower[i - 2] == "without" && words_lower[i - 1] == "doubt" {
                valence
            } else if negated_single(&words_lower[i - 2]) {
                valence * N_SCALAR
            } else {
                valence
            }
        }
        2 => {
            if (words_lower[i - 3] == "never"
                && (words_lower[i - 2] == "so" || words_lower[i - 2] == "this"))
                || words_lower[i - 1] == "so"
                || words_lower[i - 1] == "this"
            {
                valence * 1.25
            } else if words_lower[i - 3] == "without"
                && (words_lower[i - 2] == "doubt" || words_lower[i - 1] == "doubt")
            {
                valence
            } else if negated_single(&words_lower[i - 3]) {
                valence * N_SCALAR
            } else {
                valence
            }
        }
        _ => valence,
    }
}

/// Multi-word idioms/phrases (e.g. "the bomb", "kind of") whose meaning
/// isn't the sum of their lexicon parts. Only reachable at `start_i == 2`
/// (so `i >= 3`), matching upstream.
fn special_idioms_check(valence: f64, wl: &[String], i: usize) -> f64 {
    let onezero = format!("{} {}", wl[i - 1], wl[i]);
    let twoonezero = format!("{} {} {}", wl[i - 2], wl[i - 1], wl[i]);
    let twoone = format!("{} {}", wl[i - 2], wl[i - 1]);
    let threetwoone = format!("{} {} {}", wl[i - 3], wl[i - 2], wl[i - 1]);
    let threetwo = format!("{} {}", wl[i - 3], wl[i - 2]);

    let mut valence = valence;
    for seq in [&onezero, &twoonezero, &twoone, &threetwoone, &threetwo] {
        if let Some(&v) = SPECIAL_CASES.get(seq.as_str()) {
            valence = v;
            break;
        }
    }

    if wl.len() - 1 > i {
        let zeroone = format!("{} {}", wl[i], wl[i + 1]);
        if let Some(&v) = SPECIAL_CASES.get(zeroone.as_str()) {
            valence = v;
        }
    }
    if wl.len() - 1 > i + 1 {
        let zeroonetwo = format!("{} {} {}", wl[i], wl[i + 1], wl[i + 2]);
        if let Some(&v) = SPECIAL_CASES.get(zeroonetwo.as_str()) {
            valence = v;
        }
    }

    for n_gram in [&threetwoone, &threetwo, &twoone] {
        if let Some(&b) = BOOSTER_DICT.get(n_gram.as_str()) {
            valence += b;
        }
    }
    valence
}

/// "least" negates the following lexicon word, unless it's "at least" or
/// "very least" (hedges, not negation).
fn least_check(valence: f64, wl: &[String], i: usize) -> f64 {
    if i > 1 && !LEXICON.contains_key(wl[i - 1].as_str()) && wl[i - 1] == "least" {
        if wl[i - 2] != "at" && wl[i - 2] != "very" {
            return valence * N_SCALAR;
        }
    } else if i > 0 && !LEXICON.contains_key(wl[i - 1].as_str()) && wl[i - 1] == "least" {
        return valence * N_SCALAR;
    }
    valence
}

/// Words before "but" count for less, words after count for more — "but"
/// signals the speaker's real point comes after it.
fn but_check(words_lower: &[String], sentiments: &mut [f64]) {
    let Some(bi) = words_lower.iter().position(|w| w == "but") else {
        return;
    };
    for (si, s) in sentiments.iter_mut().enumerate() {
        if si < bi {
            *s *= 0.5;
        } else if si > bi {
            *s *= 1.5;
        }
    }
}

fn amplify_exclamation(text: &str) -> f64 {
    let count = text.matches('!').count().min(4);
    count as f64 * 0.292
}

fn amplify_question(text: &str) -> f64 {
    let count = text.matches('?').count();
    if count > 3 {
        0.96
    } else if count > 1 {
        count as f64 * 0.18
    } else {
        0.0
    }
}

fn punctuation_emphasis(text: &str) -> f64 {
    amplify_exclamation(text) + amplify_question(text)
}

/// Normalizes a raw summed score to `[-1, 1]` using an alpha that
/// approximates the maximum expected raw value.
fn normalize(score: f64, alpha: f64) -> f64 {
    let norm = score / (score * score + alpha).sqrt();
    norm.clamp(-1.0, 1.0)
}

fn sift_sentiment_scores(sentiments: &[f64]) -> (f64, f64, usize) {
    let mut pos_sum = 0.0;
    let mut neg_sum = 0.0;
    let mut neu_count = 0usize;
    for &s in sentiments {
        if s > 0.0 {
            pos_sum += s + 1.0;
        }
        if s < 0.0 {
            neg_sum += s - 1.0;
        }
        if s == 0.0 {
            neu_count += 1;
        }
    }
    (pos_sum, neg_sum, neu_count)
}

fn score_valence(sentiments: &[f64], text: &str) -> PolarityScores {
    if sentiments.is_empty() {
        return PolarityScores {
            neg: 0.0,
            neu: 0.0,
            pos: 0.0,
            compound: 0.0,
        };
    }

    let mut sum_s: f64 = sentiments.iter().sum();
    let punct_emph = punctuation_emphasis(text);
    if sum_s > 0.0 {
        sum_s += punct_emph;
    } else if sum_s < 0.0 {
        sum_s -= punct_emph;
    }
    let compound = normalize(sum_s, 15.0);

    let (mut pos_sum, mut neg_sum, neu_count) = sift_sentiment_scores(sentiments);
    if pos_sum > neg_sum.abs() {
        pos_sum += punct_emph;
    } else if pos_sum < neg_sum.abs() {
        neg_sum -= punct_emph;
    }

    let total = pos_sum + neg_sum.abs() + neu_count as f64;
    let pos = (pos_sum / total).abs();
    let neg = (neg_sum / total).abs();
    let neu = (neu_count as f64 / total).abs();

    PolarityScores {
        neg: round3(neg),
        neu: round3(neu),
        pos: round3(pos),
        compound: round4(compound),
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}
fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compound(text: &str) -> f64 {
        polarity_scores(text).compound
    }

    #[test]
    fn plain_positive_and_negative_text() {
        assert!(compound("This is great and wonderful.") > 0.5);
        assert!(compound("This is horrible and awful.") < -0.5);
    }

    #[test]
    fn neutral_text_is_near_zero() {
        assert!(compound("The meeting is at 3pm.").abs() < 0.2);
    }

    #[test]
    fn negation_flips_the_sign() {
        assert!(compound("This is not good.") < compound("This is good."));
    }

    #[test]
    fn booster_word_increases_magnitude() {
        assert!(compound("This is very good.") > compound("This is good."));
        assert!(compound("This is slightly good.") < compound("This is good."));
    }

    #[test]
    fn all_caps_emphasis_increases_magnitude_when_mixed_with_lowercase() {
        assert!(compound("this is GREAT") > compound("this is great"));
    }

    #[test]
    fn all_caps_emphasis_does_not_apply_when_everything_is_caps() {
        // No differential (every word is caps), so no extra boost versus the
        // same all-lowercase sentence.
        let all_caps = compound("THIS IS GREAT");
        let all_lower = compound("this is great");
        assert!((all_caps - all_lower).abs() < 1e-9);
    }

    #[test]
    fn exclamation_marks_amplify_a_nonzero_score() {
        assert!(compound("This is great!!!") > compound("This is great"));
        assert!(compound("This is horrible!!!") < compound("This is horrible"));
    }

    #[test]
    fn empty_text_scores_neutral() {
        let s = polarity_scores("");
        assert_eq!(
            s,
            PolarityScores {
                neg: 0.0,
                neu: 0.0,
                pos: 0.0,
                compound: 0.0
            }
        );
    }

    #[test]
    fn but_check_weights_the_clause_after_but_more_heavily() {
        // "but" should push the compound toward the second clause's polarity.
        let praise_then_complaint = compound("It's fine but this is terrible");
        let complaint_then_praise = compound("This is terrible but it's fine");
        assert!(praise_then_complaint < complaint_then_praise);
    }

    #[test]
    fn special_case_idiom_overrides_literal_word_meaning() {
        // "the bomb" is strongly positive slang despite containing no
        // individually-positive lexicon word on its own.
        assert!(compound("this is the bomb") > 0.5);
    }

    #[test]
    fn least_negates_unless_at_least_or_very_least() {
        assert!(compound("this is the least good option") < 0.0);
        assert!(compound("this is at least good") >= 0.0);
    }

    #[test]
    fn kind_of_dampens_like_a_booster_not_like_a_standalone_word() {
        // "kind" alone can carry positive valence; "kind of" should read as
        // a hedge/dampener, not as praise.
        assert!(compound("this is kind of good") < compound("this is good"));
    }

    // The tests below call the private helper functions directly with
    // hand-constructed word lists, to reach specific branches precisely
    // rather than hoping an English sentence happens to tokenize the right
    // way. All of these mirror real (if sometimes awkward) VADER behavior —
    // none are contrived to be impossible in real text.

    fn words(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_immediately_before_a_lexicon_word_zeroes_its_own_valence() {
        let w = words(&["no", "good"]);
        // "no" itself, followed by another lexicon word ("good") — its own
        // valence is zeroed so it acts as pure negation instead.
        assert_eq!(sentiment_valence(&w, &w, 0, false), 0.0);
    }

    #[test]
    fn no_one_word_back_negates_the_current_lexicon_word() {
        let w = words(&["no", "good"]);
        let expected = LEXICON["good"] * N_SCALAR;
        assert!((sentiment_valence(&w, &w, 1, false) - expected).abs() < 1e-9);
    }

    #[test]
    fn no_three_words_back_negates_only_via_the_or_nor_pattern() {
        // The third `no_negates` disjunct only fires through "X or/nor no
        // <word>" — distinct from (and only reachable when) the closer
        // one-word-back and two-word-back checks are both false. "or" isn't
        // itself a negation word, so this is the only source of negation in
        // that case.
        let expected_or = LEXICON["good"] * N_SCALAR;
        let or_form = words(&["no", "x", "or", "good"]);
        assert!((sentiment_valence(&or_form, &or_form, 3, false) - expected_or).abs() < 1e-9);

        // "nor" is *also* a one-word-back negation word in its own right
        // (see NEGATE), so it compounds: the explicit no-3-back rule negates
        // once, and the generic distance-1 negation check negates again —
        // a real interaction between two independent mechanisms, not a bug.
        let expected_nor = LEXICON["good"] * N_SCALAR * N_SCALAR;
        let nor_form = words(&["no", "x", "nor", "good"]);
        assert!((sentiment_valence(&nor_form, &nor_form, 3, false) - expected_nor).abs() < 1e-9);
    }

    #[test]
    fn stacked_booster_words_at_distance_two_and_three_both_apply_scaled() {
        // "extremely" (distance 3), "really" (distance 2), "so" (distance 1)
        // all boost "good" — exercises the 0.95x (distance 2) and 0.9x
        // (distance 3) scaling branches, on top of the unscaled distance-1 case.
        let w = words(&["extremely", "really", "so", "good"]);
        let result = sentiment_valence(&w, &w, 3, false);
        assert!(
            result > 2.5,
            "expected a heavily boosted valence, got {result}"
        );
    }

    #[test]
    fn sentiment_valence_all_caps_extends_a_negative_lexicon_word() {
        let words = vec!["this".to_string(), "is".to_string(), "BAD".to_string()];
        let words_lower = vec!["this".to_string(), "is".to_string(), "bad".to_string()];
        let plain = sentiment_valence(&words_lower, &words_lower, 2, false);
        let capped = sentiment_valence(&words, &words_lower, 2, true);
        assert!((capped - (plain - C_INCR)).abs() < 1e-9);
    }

    #[test]
    fn negation_check_out_of_range_start_i_is_a_documented_no_op() {
        // start_i is always 0, 1, or 2 through the real call path (the
        // enclosing loop is `for start_i in 0..3`); this pins the match's
        // required-for-exhaustiveness catch-all to a stated behavior
        // (leave valence untouched) rather than leaving it silently unverified.
        let w = words(&["x", "y", "z", "w"]);
        assert_eq!(negation_check(1.0, &w, 99, 3), 1.0);
    }

    #[test]
    fn scalar_inc_dec_all_caps_boosts_a_positive_valence() {
        let boosted = scalar_inc_dec("VERY", 1.9, true);
        let plain = scalar_inc_dec("very", 1.9, true);
        assert!((boosted - (plain + C_INCR)).abs() < 1e-9);
    }

    #[test]
    fn scalar_inc_dec_all_caps_extends_a_negative_valence() {
        let boosted = scalar_inc_dec("VERY", -1.9, true);
        let plain = scalar_inc_dec("very", -1.9, true);
        assert!((boosted - (plain - C_INCR)).abs() < 1e-9);
    }

    #[test]
    fn negation_check_never_so_or_this_amplifies_instead_of_negating() {
        let so_form = words(&["never", "so", "x"]);
        assert_eq!(negation_check(1.0, &so_form, 1, 2), 1.25);
        let this_form = words(&["never", "this", "x"]);
        assert_eq!(negation_check(1.0, &this_form, 1, 2), 1.25);
    }

    #[test]
    fn negation_check_never_so_or_this_at_distance_three_amplifies() {
        let so_form = words(&["never", "so", "x", "y"]);
        assert_eq!(negation_check(1.0, &so_form, 2, 3), 1.25);
        let this_form = words(&["never", "this", "x", "y"]);
        assert_eq!(negation_check(1.0, &this_form, 2, 3), 1.25);
    }

    #[test]
    fn negation_check_without_doubt_at_distance_two_leaves_valence_alone() {
        let w = words(&["without", "doubt", "x"]);
        assert_eq!(negation_check(1.0, &w, 1, 2), 1.0);
    }

    #[test]
    fn negation_check_plain_negation_at_distance_two() {
        let w = words(&["not", "x", "y"]);
        assert_eq!(negation_check(1.0, &w, 1, 2), N_SCALAR);
    }

    #[test]
    fn negation_check_without_doubt_at_distance_three_leaves_valence_alone() {
        let w = words(&["without", "x", "doubt", "y"]);
        assert_eq!(negation_check(1.0, &w, 2, 3), 1.0);
    }

    #[test]
    fn negation_check_plain_negation_at_distance_three() {
        let w = words(&["rarely", "x", "y", "z"]);
        assert_eq!(negation_check(1.0, &w, 2, 3), N_SCALAR);
    }

    #[test]
    fn special_idioms_check_matches_a_bigram_immediately_after_the_word() {
        let w = words(&["x", "y", "z", "bad", "ass"]);
        assert_eq!(special_idioms_check(0.0, &w, 3), 1.5);
    }

    #[test]
    fn special_idioms_check_matches_a_trigram_starting_at_the_word() {
        let w = words(&["x", "y", "z", "kiss", "of", "death"]);
        assert_eq!(special_idioms_check(0.0, &w, 3), -1.5);
    }

    #[test]
    fn least_check_negates_when_least_is_the_very_first_token() {
        let w = words(&["least", "good"]);
        assert_eq!(least_check(1.0, &w, 1), N_SCALAR);
    }

    #[test]
    fn amplify_question_four_or_more_marks_is_a_flat_amplifier() {
        assert_eq!(amplify_question("????"), 0.96);
    }

    #[test]
    fn amplify_question_two_or_three_marks_scales_with_count() {
        assert_eq!(amplify_question("??"), 2.0 * 0.18);
        assert_eq!(amplify_question("???"), 3.0 * 0.18);
    }
}
