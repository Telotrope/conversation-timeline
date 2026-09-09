//! A faithful Rust port of the core VADER sentiment algorithm, used in place
//! of the original AFINN lexicon (see [`crate::flags::anger`] and the
//! license discussion in the migration plan's C2).

pub mod algorithm;
pub mod lexicon;
pub mod tokenize;

pub use algorithm::{polarity_scores, PolarityScores};
