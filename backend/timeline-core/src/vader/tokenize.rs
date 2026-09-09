//! Tokenization helpers for the VADER algorithm: whitespace-splitting that
//! preserves contractions and most emoticons, and the ALL-CAPS-emphasis
//! detection used to boost a sentiment-laden word written in caps.
//! Port of `SentiText`/`allcap_differential` in the original `vaderSentiment.py`.

/// Python's `string.punctuation`.
const PUNCTUATION: &str = "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

/// Strips leading/trailing punctuation from `token`, unless doing so would
/// leave 2 or fewer characters — in which case the token is probably an
/// emoticon (e.g. `":)"` stripped would become `""`), so it's returned as-is.
fn strip_punc_if_word(token: &str) -> String {
    let stripped = token.trim_matches(|c| PUNCTUATION.contains(c));
    if stripped.chars().count() <= 2 {
        token.to_string()
    } else {
        stripped.to_string()
    }
}

/// Splits `text` on whitespace and strips leading/trailing punctuation from
/// each resulting token.
pub fn words_and_emoticons(text: &str) -> Vec<String> {
    text.split_whitespace().map(strip_punc_if_word).collect()
}

/// Python's `str.isupper()`: at least one cased character, and every cased
/// character is uppercase.
pub fn is_all_upper(word: &str) -> bool {
    let mut saw_cased = false;
    for c in word.chars() {
        if c.is_uppercase() {
            saw_cased = true;
        } else if c.is_lowercase() {
            return false;
        }
    }
    saw_cased
}

/// True if *some but not all* of `words` are ALL CAPS — the signal used to
/// decide whether writing a word in caps was meaningful emphasis (mixed
/// case elsewhere) versus the whole message just being shouted in caps
/// throughout (no differential, so no extra signal).
pub fn allcap_differential(words: &[String]) -> bool {
    let allcap_words = words.iter().filter(|w| is_all_upper(w)).count();
    let cap_differential = words.len().saturating_sub(allcap_words);
    cap_differential > 0 && cap_differential < words.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_leading_and_trailing_punctuation() {
        assert_eq!(words_and_emoticons("Hello, world!"), vec!["Hello", "world"]);
    }

    #[test]
    fn preserves_short_emoticons() {
        assert_eq!(words_and_emoticons(":) great"), vec![":)", "great"]);
    }

    #[test]
    fn preserves_contractions() {
        assert_eq!(words_and_emoticons("didn't work"), vec!["didn't", "work"]);
    }

    #[test]
    fn is_all_upper_requires_at_least_one_cased_char() {
        assert!(!is_all_upper("123"));
        assert!(!is_all_upper(""));
    }

    #[test]
    fn is_all_upper_true_for_all_caps_word() {
        assert!(is_all_upper("WRONG"));
        assert!(is_all_upper("I"));
    }

    #[test]
    fn is_all_upper_false_if_any_lowercase_present() {
        assert!(!is_all_upper("Wrong"));
        assert!(!is_all_upper("wRONG"));
    }

    #[test]
    fn allcap_differential_true_when_some_but_not_all_caps() {
        let words: Vec<String> = ["this", "is", "WRONG"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(allcap_differential(&words));
    }

    #[test]
    fn allcap_differential_false_when_all_caps() {
        let words: Vec<String> = ["THIS", "IS", "WRONG"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!allcap_differential(&words));
    }

    #[test]
    fn allcap_differential_false_when_none_caps() {
        let words: Vec<String> = ["this", "is", "fine"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!allcap_differential(&words));
    }

    #[test]
    fn allcap_differential_false_on_empty_input() {
        assert!(!allcap_differential(&[]));
    }
}
