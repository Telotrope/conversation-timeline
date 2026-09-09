//! Static data for the VADER sentiment algorithm: the word-valence lexicon,
//! the booster/dampener word list, the negation word list, and the small set
//! of multi-word special-case phrases. Transcribed from the original,
//! MIT-licensed `vaderSentiment` project (C.J. Hutto, 2016) — see
//! `data/VADER_LEXICON_LICENSE.txt`. `data/vader_lexicon.txt` is the
//! upstream lexicon file, embedded verbatim.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

const LEXICON_RAW: &str = include_str!("../../data/vader_lexicon.txt");

/// Empirically derived mean sentiment intensity increase for booster words.
pub const B_INCR: f64 = 0.293;
pub const B_DECR: f64 = -0.293;
/// Empirically derived mean sentiment intensity increase for ALL-CAPS
/// emphasis of a sentiment-laden word.
pub const C_INCR: f64 = 0.733;
/// Multiplier applied to a valence when it's negated.
pub const N_SCALAR: f64 = -0.74;

pub static LEXICON: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    LEXICON_RAW
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, '\t');
            let word = parts.next().expect("lexicon line has a word column");
            let score: f64 = parts
                .next()
                .expect("lexicon line has a score column")
                .parse()
                .expect("lexicon score column is a float");
            (word, score)
        })
        .collect()
});

pub static NEGATE: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "aint",
        "arent",
        "cannot",
        "cant",
        "couldnt",
        "darent",
        "didnt",
        "doesnt",
        "ain't",
        "aren't",
        "can't",
        "couldn't",
        "daren't",
        "didn't",
        "doesn't",
        "dont",
        "hadnt",
        "hasnt",
        "havent",
        "isnt",
        "mightnt",
        "mustnt",
        "neither",
        "don't",
        "hadn't",
        "hasn't",
        "haven't",
        "isn't",
        "mightn't",
        "mustn't",
        "neednt",
        "needn't",
        "never",
        "none",
        "nope",
        "nor",
        "not",
        "nothing",
        "nowhere",
        "oughtnt",
        "shant",
        "shouldnt",
        "uhuh",
        "wasnt",
        "werent",
        "oughtn't",
        "shan't",
        "shouldn't",
        "uh-uh",
        "wasn't",
        "weren't",
        "without",
        "wont",
        "wouldnt",
        "won't",
        "wouldn't",
        "rarely",
        "seldom",
        "despite",
    ]
    .into_iter()
    .collect()
});

pub static BOOSTER_DICT: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    for w in [
        "absolutely",
        "amazingly",
        "awfully",
        "completely",
        "considerable",
        "considerably",
        "decidedly",
        "deeply",
        "effing",
        "enormous",
        "enormously",
        "entirely",
        "especially",
        "exceptional",
        "exceptionally",
        "extreme",
        "extremely",
        "fabulously",
        "flipping",
        "flippin",
        "frackin",
        "fracking",
        "fricking",
        "frickin",
        "frigging",
        "friggin",
        "fully",
        "fuckin",
        "fucking",
        "fuggin",
        "fugging",
        "greatly",
        "hella",
        "highly",
        "hugely",
        "incredible",
        "incredibly",
        "intensely",
        "major",
        "majorly",
        "more",
        "most",
        "particularly",
        "purely",
        "quite",
        "really",
        "remarkably",
        "so",
        "substantially",
        "thoroughly",
        "total",
        "totally",
        "tremendous",
        "tremendously",
        "uber",
        "unbelievably",
        "unusually",
        "utter",
        "utterly",
        "very",
    ] {
        m.insert(w, B_INCR);
    }
    for w in [
        "almost",
        "barely",
        "hardly",
        "just enough",
        "kind of",
        "kinda",
        "kindof",
        "kind-of",
        "less",
        "little",
        "marginal",
        "marginally",
        "occasional",
        "occasionally",
        "partly",
        "scarce",
        "scarcely",
        "slight",
        "slightly",
        "somewhat",
        "sort of",
        "sorta",
        "sortof",
        "sort-of",
    ] {
        m.insert(w, B_DECR);
    }
    m
});

/// Multi-word phrases containing lexicon words whose combined meaning isn't
/// the sum of their parts (e.g. "the bomb" is strongly positive slang, not
/// literally about an explosive).
pub static SPECIAL_CASES: LazyLock<HashMap<&'static str, f64>> = LazyLock::new(|| {
    [
        ("the shit", 3.0),
        ("the bomb", 3.0),
        ("bad ass", 1.5),
        ("badass", 1.5),
        ("bus stop", 0.0),
        ("yeah right", -2.0),
        ("kiss of death", -1.5),
        ("to die for", 3.0),
        ("beating heart", 3.1),
        ("broken heart", -2.9),
    ]
    .into_iter()
    .collect()
});
