//! Reading attributes out of a DynamoDB row, with no silent defaults: a
//! missing attribute and an attribute of the wrong type are each reported as
//! a distinct error naming the attribute and the row, never replaced with an
//! empty string, zero or `false`. See the migration plan's §V2c.
//!
//! Every row these adapters read was written by our own code, which always
//! writes the attributes read with the `required_*` functions. So a missing
//! or malformed one means the row was corrupted or written by something
//! else, and the operator needs to know which row and which attribute.
//!
//! Error text describes what was found by *type* ("a number", "a list"), not
//! by content: stored values include conversation names taken from users'
//! uploads, and those don't belong in logs. The one exception is a value
//! that should have been an id, shown escaped and cut to 64 characters,
//! because seeing the bad id is what makes the error actionable.

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use timeline_core::ports::errors::StoreError;

pub(crate) type Item = HashMap<String, AttributeValue>;

/// Stored data that can't be read: `StoreError::Damaged`, so the page can
/// tell it apart from the store failing to answer.
pub(crate) fn invalid_data(msg: impl Into<String>) -> StoreError {
    StoreError::Damaged(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        msg.into(),
    )))
}

/// The row's sort key, for error messages. Every row has one (the table's
/// key schema requires it), but this is only a label, so a missing one is
/// shown rather than treated as a second error.
fn row_label(item: &Item) -> String {
    match item.get("sk").and_then(|v| v.as_s().ok()) {
        Some(sk) => format!("row with sk {:?}", shorten(sk)),
        // Currently unreachable: the key schema makes `sk` required, so every
        // row DynamoDB returns has one. Kept as a backstop if a caller ever
        // labels a row that didn't come from a table read.
        None => "row with no sk".to_string(),
    }
}

/// Escaped and cut to 64 characters, so a bad value can't flood a log line
/// or smuggle control characters into it.
fn shorten(value: &str) -> String {
    let escaped: String = value.escape_debug().collect();
    if escaped.chars().count() > 64 {
        format!("{}…", escaped.chars().take(64).collect::<String>())
    } else {
        escaped
    }
}

fn type_name(value: &AttributeValue) -> &'static str {
    match value {
        AttributeValue::S(_) => "a string",
        AttributeValue::N(_) => "a number",
        AttributeValue::Bool(_) => "a true/false value",
        AttributeValue::L(_) => "a list",
        AttributeValue::M(_) => "a map",
        AttributeValue::Null(_) => "null",
        _ => "an unsupported type",
    }
}

fn missing(item: &Item, name: &str) -> StoreError {
    invalid_data(format!(
        "{}: attribute `{name}` is missing",
        row_label(item)
    ))
}

fn wrong_type(item: &Item, name: &str, expected: &str, found: &AttributeValue) -> StoreError {
    invalid_data(format!(
        "{}: attribute `{name}` should be {expected} but is {}",
        row_label(item),
        type_name(found)
    ))
}

fn required<'a>(item: &'a Item, name: &str) -> Result<&'a AttributeValue, StoreError> {
    item.get(name).ok_or_else(|| missing(item, name))
}

pub(crate) fn required_string<'a>(item: &'a Item, name: &str) -> Result<&'a str, StoreError> {
    let value = required(item, name)?;
    value
        .as_s()
        .map(String::as_str)
        .map_err(|found| wrong_type(item, name, "a string", found))
}

/// A whole number of 0 or more, such as a message count.
pub(crate) fn required_count(item: &Item, name: &str) -> Result<usize, StoreError> {
    let value = required(item, name)?;
    let text = value
        .as_n()
        .map_err(|found| wrong_type(item, name, "a number", found))?;
    text.parse::<usize>().map_err(|_| {
        invalid_data(format!(
            "{}: attribute `{name}` should be a whole number of 0 or more but is {:?}",
            row_label(item),
            shorten(text)
        ))
    })
}

/// A list in which every entry is a string holding a valid id. One bad entry
/// fails the whole read: dropping it would silently lose data.
pub(crate) fn required_id_list(item: &Item, name: &str) -> Result<Vec<uuid::Uuid>, StoreError> {
    let value = required(item, name)?;
    let entries = value
        .as_l()
        .map_err(|found| wrong_type(item, name, "a list", found))?;
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let text = entry.as_s().map_err(|found| {
                wrong_type(item, &format!("{name}[{index}]"), "a string", found)
            })?;
            text.parse::<uuid::Uuid>().map_err(|_| {
                invalid_data(format!(
                    "{}: attribute `{name}[{index}]` should be an id but is {:?}",
                    row_label(item),
                    shorten(text)
                ))
            })
        })
        .collect()
}

/// `None` when absent -- for attributes where absence is meaningful, such as
/// a flag nobody has set yet. Present but not true/false is an error.
pub(crate) fn optional_bool(item: &Item, name: &str) -> Result<Option<bool>, StoreError> {
    match item.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_bool()
            .copied()
            .map(Some)
            .map_err(|found| wrong_type(item, name, "a true/false value", found)),
    }
}

/// `None` when absent, as for `optional_bool`. Present but not a string is
/// an error.
pub(crate) fn optional_string<'a>(
    item: &'a Item,
    name: &str,
) -> Result<Option<&'a str>, StoreError> {
    match item.get(name) {
        None => Ok(None),
        Some(value) => value
            .as_s()
            .map(|s| Some(s.as_str()))
            .map_err(|found| wrong_type(item, name, "a string", found)),
    }
}

/// An attribute holding JSON text written by `json_attribute`, read back as
/// `T`. The error says where the text stopped making sense (line, column and
/// kind of problem), never what it contained: these values hold names
/// people typed.
pub(crate) fn required_json<T: serde::de::DeserializeOwned>(
    item: &Item,
    name: &str,
) -> Result<T, StoreError> {
    let text = required_string(item, name)?;
    serde_json::from_str(text).map_err(|e| {
        invalid_data(format!(
            "{}: attribute `{name}` is not the expected JSON ({:?} problem at line {}, column {})",
            row_label(item),
            e.classify(),
            e.line(),
            e.column()
        ))
    })
}

/// `value` as JSON text, for `required_json` to read back.
pub(crate) fn json_attribute<T: serde::Serialize>(value: &T) -> AttributeValue {
    // Unreachable backstop: every type stored this way is plain data
    // (strings, numbers, dates, lists and tagged variants), which
    // serde_json always serializes.
    AttributeValue::S(serde_json::to_string(value).expect("plain data serializes to JSON"))
}
