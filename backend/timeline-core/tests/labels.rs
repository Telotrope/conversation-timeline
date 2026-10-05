//! The typed labels (plan docs/plans/2026-10-05-screen-flow.md §8a): each is
//! cleaned once, when read, the same way the page cleans text.

use timeline_core::labels::{
    clean_label, AiName, FileName, LabelError, PersonName, ServiceName, LABEL_CAP,
};

#[test]
fn whitespace_runs_become_one_space_and_the_ends_are_trimmed() {
    assert_eq!(
        clean_label("  Ada \t\n Lovelace  ").unwrap(),
        "Ada Lovelace"
    );
}

#[test]
fn control_and_invisible_characters_are_removed() {
    // NUL, escape, zero-width space, right-to-left override, byte order mark.
    let raw = "A\u{0}d\u{1b}a\u{200b} \u{202e}L\u{feff}";
    assert_eq!(clean_label(raw).unwrap(), "Ada L");
}

#[test]
fn text_with_nothing_visible_is_refused() {
    assert_eq!(clean_label(" \u{200b}\t\u{0} "), Err(LabelError::Empty));
    assert_eq!(clean_label(""), Err(LabelError::Empty));
    assert_eq!(LabelError::Empty.to_string(), "must not be empty");
}

#[test]
fn long_text_is_cut_to_the_cap_in_characters_never_splitting_one() {
    let emoji = "🙂".repeat(LABEL_CAP + 5);
    let cleaned = clean_label(&emoji).unwrap();
    assert_eq!(cleaned.chars().count(), LABEL_CAP);
    assert!(cleaned.chars().all(|c| c == '🙂'));
}

#[test]
fn every_label_type_cleans_on_parse_and_shows_its_text() {
    assert_eq!(PersonName::parse(" Ada ").unwrap().as_str(), "Ada");
    assert_eq!(AiName::parse("Le Chat").unwrap().to_string(), "Le Chat");
    assert_eq!(ServiceName::parse("Tactiq").unwrap().as_str(), "Tactiq");
    assert_eq!(
        FileName::parse("a\u{200b}.json").unwrap().as_str(),
        "a.json"
    );
    assert_eq!(PersonName::parse("  "), Err(LabelError::Empty));
}

#[test]
fn reading_a_label_from_json_cleans_it_and_refuses_an_empty_one() {
    let name: PersonName = serde_json::from_str(r#""  Ada\u0000  ""#).unwrap();
    assert_eq!(name.as_str(), "Ada");
    assert_eq!(serde_json::to_string(&name).unwrap(), r#""Ada""#);
    let err = serde_json::from_str::<FileName>(r#"" ""#).unwrap_err();
    assert!(err.to_string().contains("must not be empty"), "{err}");
    let back: String = name.into();
    assert_eq!(back, "Ada");
}
