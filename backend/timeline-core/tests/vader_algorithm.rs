//! Black-box tests for the VADER sentiment algorithm, calling only
//! `polarity_scores` — the crate's public entry point. Every internal
//! mechanism (negation, boosters, ALL-CAPS emphasis, special-case idioms,
//! "least", punctuation emphasis) is proven here through real sentences that
//! actually exercise it, rather than by calling the private functions in
//! `src/vader/algorithm.rs` directly. See that file's doc comments for *why*
//! each mechanism exists; these tests prove *that* it works, from outside.

use timeline_core::vader::{polarity_scores, PolarityScores};

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
fn question_marks_amplify_monotonically_up_to_the_four_mark_ceiling() {
    // Exercises `amplify_question`'s two branches (linear 2-3, flat at 4+)
    // through observable behavior: more question marks after a negative
    // statement reads as more emphatic, whatever the exact internal formula.
    let one = compound("Is this really that bad?");
    let two = compound("Is this really that bad??");
    let three = compound("Is this really that bad???");
    let four = compound("Is this really that bad????");
    assert!(one > two, "2 marks should read more negative than 1");
    assert!(two > three, "3 marks should read more negative than 2");
    assert!(three > four, "4+ marks should read more negative than 3");
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
fn special_case_bigram_immediately_after_the_lexicon_word() {
    // "bad ass" (positive slang) despite "bad" alone being strongly
    // negative — the idiom check looks one word *ahead*, not just behind.
    assert!(compound("this is really quite bad ass") > 0.0);
}

#[test]
fn special_case_trigram_starting_at_the_lexicon_word() {
    // "kiss of death" (negative idiom) despite "kiss" alone being positive.
    assert!(compound("this is really quite kiss of death") < 0.0);
}

#[test]
fn least_negates_unless_at_least_or_very_least() {
    assert!(compound("this is the least good option") < 0.0);
    assert!(compound("this is at least good") >= 0.0);
}

#[test]
fn least_negates_even_as_the_very_first_word() {
    assert!(compound("least good option available") < 0.0);
}

#[test]
fn kind_of_dampens_like_a_booster_not_like_a_standalone_word() {
    // "kind" alone can carry positive valence; "kind of" should read as a
    // hedge/dampener, not as praise.
    assert!(compound("this is kind of good") < compound("this is good"));
}

#[test]
fn no_immediately_before_a_word_negates_it() {
    // "no good" reads negative, not as "good" plus a separately-scored "no".
    assert!(compound("no good") < compound("good"));
}

#[test]
fn no_one_word_back_negates_the_following_word() {
    assert!(compound("there is no good option here") < compound("there is a good option here"));
}

#[test]
fn no_three_words_back_negates_via_the_or_nor_pattern() {
    // "no X or/nor <word>" negates <word>, even with a word in between.
    assert!(compound("no way or good result") < compound("way or good result"));
    assert!(compound("no way nor good result") < compound("way or good result"));
}

#[test]
fn stacked_booster_words_compound_their_effect() {
    // "extremely" (distance 3), "really" (distance 2), "so" (distance 1) —
    // all three boosting "good" should push the score well past what any
    // single booster achieves alone.
    let triple = compound("extremely really so good");
    let single = compound("so good");
    assert!(
        triple > single,
        "stacked boosters ({triple}) should exceed a single one ({single})"
    );
}

#[test]
fn all_caps_on_a_negative_word_extends_it_further_negative() {
    assert!(compound("this is BAD") < compound("this is bad"));
}

#[test]
fn all_caps_on_a_booster_word_extends_its_effect() {
    assert!(compound("this is VERY good") > compound("this is very good"));
    assert!(compound("this is VERY bad") < compound("this is very bad"));
}

#[test]
fn never_so_or_this_amplifies_rather_than_negates() {
    // "never" ordinarily negates (see `no_one_word_back_negates_...`
    // above's cousin behavior), but "never so"/"never this" is a
    // fixed idiom meaning emphasis, not negation — it should score more
    // positively than plain negation of the same word, not less.
    assert!(compound("it was never so good before") > compound("it was not so good before"));
    assert!(compound("it was never this good before") > compound("it was not this good before"));
}

#[test]
fn never_so_amplifies_at_distance_three_too() {
    assert!(compound("never so very good") > compound("not so very good"));
}

#[test]
fn never_this_amplifies_at_distance_three_too() {
    // Isolates the "this" alternative of the distance-3 "never" clause,
    // distinct from the "so" alternative tested above.
    assert!(compound("never this very good") > compound("not this very good"));
}

#[test]
fn without_doubt_does_not_negate_despite_containing_a_negative_word() {
    // "doubt" is itself a negative lexicon word, but "without doubt" is an
    // idiom meaning certainty, not negation of what follows.
    assert!(compound("this is without doubt good") > compound("this is not good"));
}

#[test]
fn without_a_doubt_does_not_negate_at_distance_three_either() {
    assert!(compound("this is without a doubt good") > compound("this is not good"));
}

#[test]
fn plain_negation_words_at_distance_two_and_three_still_negate() {
    // "not" two words back, and again three words back — distinct from the
    // "never so"/"without doubt" idioms above, which look similar but don't
    // negate.
    assert!(compound("that is not really good") < compound("that is really good"));
    assert!(compound("that is not at all good") < compound("that is good"));
}
