//! Black-box tests for the VADER tokenization helpers, calling only the
//! crate's public API.

use timeline_core::vader::tokenize::{allcap_differential, is_all_upper, words_and_emoticons};

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
