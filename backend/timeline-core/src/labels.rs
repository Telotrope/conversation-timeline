//! Short pieces of text a person types to label something: a participant's
//! name, an AI's name, a transcription service, a file's name (plan
//! `docs/plans/2026-10-05-screen-flow.md` §8a). Each is its own newtype, so
//! a file name can't be passed where a person's name belongs, and each is
//! cleaned once, when it is read, by [`clean_label`].
//!
//! The cleaning matches the page's own (`cleanText` in
//! `frontend/core/activity-event.js`): runs of whitespace become one space,
//! control and invisible formatting characters (Unicode `Cc` and `Cf`:
//! NUL, escape, zero-width space and joiner, right-to-left override, byte
//! order mark, soft hyphen) are removed, the ends are trimmed, and the text
//! is cut to [`LABEL_CAP`] characters. Text that is empty after cleaning is
//! refused.

use std::fmt;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// The most characters a label keeps (characters, not bytes, so an emoji
/// is never split).
pub const LABEL_CAP: usize = 200;

fn whitespace() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").expect("a valid pattern"))
}

fn invisible() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"[\p{Cc}\p{Cf}]").expect("a valid pattern"))
}

/// Why a label was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelError {
    /// Nothing was left once whitespace and invisible characters were removed.
    Empty,
}

impl fmt::Display for LabelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LabelError::Empty => write!(f, "must not be empty"),
        }
    }
}

impl std::error::Error for LabelError {}

/// Cleans a label as described in the module documentation, or refuses it
/// when nothing is left.
pub fn clean_label(raw: &str) -> Result<String, LabelError> {
    let spaced = whitespace().replace_all(raw, " ");
    let visible = invisible().replace_all(&spaced, "");
    let trimmed = visible.trim();
    if trimmed.is_empty() {
        return Err(LabelError::Empty);
    }
    Ok(trimmed.chars().take(LABEL_CAP).collect())
}

macro_rules! label_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Cleans `raw` ([`clean_label`]) into this label, or refuses it.
            pub fn parse(raw: &str) -> Result<Self, LabelError> {
                clean_label(raw).map(Self)
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = LabelError;
            fn try_from(raw: String) -> Result<Self, LabelError> {
                Self::parse(&raw)
            }
        }

        impl From<$name> for String {
            fn from(label: $name) -> String {
                label.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

label_type!(
    /// A human participant's name, typed freely.
    PersonName
);
label_type!(
    /// The name of an AI participant that isn't one of the named kinds.
    AiName
);
label_type!(
    /// A transcription service that isn't one of the named ones.
    ServiceName
);
label_type!(
    /// The name of an uploaded file, as the browser reported it.
    FileName
);
