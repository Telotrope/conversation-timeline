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

    for distance in Distance::ALL {
        let offset = distance.offset();
        if i >= offset && !LEXICON.contains_key(words_lower[i - offset].as_str()) {
            let mut s = scalar_inc_dec(&words[i - offset], valence, is_cap_diff);
            match distance {
                Distance::Two if s != 0.0 => s *= 0.95,
                Distance::Three if s != 0.0 => s *= 0.9,
                _ => {}
            }
            valence += s;
            valence = negation_check(valence, words_lower, distance, i);
            if distance == Distance::Three {
                valence = special_idioms_check(valence, words_lower, i);
            }
        }
    }

    least_check(valence, words_lower, i)
}

/// How many words back from the current lexicon word a modifier (booster,
/// negation, idiom) is being checked. A closed, 3-value set — not a `usize`
/// — so there is no "impossible" 4th case to match against: the type itself
/// rules it out, rather than needing a wildcard match arm that could never
/// actually be reached by any real input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Distance {
    One,
    Two,
    Three,
}

impl Distance {
    const ALL: [Distance; 3] = [Distance::One, Distance::Two, Distance::Three];

    fn offset(self) -> usize {
        match self {
            Distance::One => 1,
            Distance::Two => 2,
            Distance::Three => 3,
        }
    }
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

fn negation_check(valence: f64, words_lower: &[String], distance: Distance, i: usize) -> f64 {
    match distance {
        Distance::One => {
            if negated_single(&words_lower[i - 1]) {
                valence * N_SCALAR
            } else {
                valence
            }
        }
        Distance::Two => {
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
        Distance::Three => {
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
    }
}

/// Multi-word idioms/phrases (e.g. "the bomb", "kind of") whose meaning
/// isn't the sum of their lexicon parts. Only reachable at `Distance::Three`
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
