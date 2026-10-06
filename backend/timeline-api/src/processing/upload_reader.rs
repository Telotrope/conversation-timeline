//! Reading an upload (plan
//! `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §7, §7b, §8b).
//!
//! The page sends its file gzip-compressed; tests and older pages send it
//! plain. A gzip upload (it starts with gzip's two marker bytes) is
//! decompressed with flate2 (MIT OR Apache-2.0); anything else is read as
//! plain JSON.
//!
//! The JSON is parsed as a stream, one conversation at a time (serde's
//! sequence access over the reader), and each conversation is turned into
//! what is kept of it ([`keep_conversation`]) as soon as it is parsed, so the
//! large parts never kept (tool results, thinking) are dropped at once and
//! no one step grows with the file. How many bytes have been read is
//! counted as it goes, for the progress row.
//!
//! The two accepted shapes are those of `timeline_core::format`: a bare
//! array of conversations (a fresh export: its retried resends are removed)
//! or `{"conversations": [...]}` (an annotated download this tool wrote).

use std::cell::Cell;
use std::fmt;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserializer;
use timeline_core::keep::{keep_conversation, KeepError, Kept};
use timeline_core::model::Conversation;
use timeline_core::FormatError;

/// gzip's first two bytes.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Why an upload couldn't be read. Turned into a `ProcessingError`, whose
/// wording names each case.
#[derive(Debug)]
pub enum ReadError {
    /// It started like gzip but didn't decompress.
    Decompress(std::io::Error),
    NotUtf8(std::str::Utf8Error),
    Format(FormatError),
    Keep(KeepError),
}

/// Decompresses `bytes` if they are gzip; returns them as they are
/// otherwise.
pub fn decompress(bytes: Vec<u8>) -> Result<Vec<u8>, ReadError> {
    if !bytes.starts_with(&GZIP_MAGIC) {
        return Ok(bytes);
    }
    let mut plain = Vec::new();
    flate2::read::GzDecoder::new(bytes.as_slice())
        .read_to_end(&mut plain)
        .map_err(ReadError::Decompress)?;
    Ok(plain)
}

/// Counts the bytes read through it.
struct Counting<'a> {
    inner: &'a [u8],
    read: Arc<AtomicU64>,
}

impl Read for Counting<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.read.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Every conversation of the upload, kept; `bytes_read` counts the parse's
/// progress through `plain`.
pub fn read_conversations(
    plain: &[u8],
    bytes_read: Arc<AtomicU64>,
) -> Result<Vec<Kept>, ReadError> {
    std::str::from_utf8(plain).map_err(ReadError::NotUtf8)?;
    let mut deserializer = serde_json::Deserializer::from_reader(Counting {
        inner: plain,
        read: bytes_read,
    });
    let marks = Marks::default();
    let result = (&mut deserializer)
        .deserialize_any(TopLevel { marks: &marks })
        .and_then(|kept| deserializer.end().map(|()| kept));
    result.map_err(|e| {
        if let Some(keep) = marks.failure.take() {
            return ReadError::Keep(keep);
        }
        ReadError::Format(match e.classify() {
            serde_json::error::Category::Data if marks.wrong_shape.get() => {
                FormatError::UnrecognizedShape
            }
            serde_json::error::Category::Data => FormatError::InvalidConversation(e),
            _ => FormatError::InvalidJson(e),
        })
    })
}

/// What the parse noticed on its way, to say why it failed.
#[derive(Default)]
struct Marks {
    /// Keeping a conversation failed.
    failure: Cell<Option<KeepError>>,
    /// The document is neither of the two accepted shapes.
    wrong_shape: Cell<bool>,
}

/// The document's top: an array of conversations, or an object holding one
/// under `conversations`. Anything else reaches serde's default for its
/// type, an error, and is marked as the wrong shape.
struct TopLevel<'a> {
    marks: &'a Marks,
}

impl<'de> Visitor<'de> for TopLevel<'_> {
    type Value = Vec<Kept>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.marks.wrong_shape.set(true);
        write!(
            f,
            "an array of conversations or an object with a conversations array"
        )
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        keep_each(seq, false, self.marks)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut kept = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "conversations" && kept.is_none() {
                kept = Some(map.next_value_seed(Conversations { marks: self.marks })?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        kept.ok_or_else(|| {
            self.marks.wrong_shape.set(true);
            serde::de::Error::custom("no conversations array")
        })
    }
}

/// The `conversations` value of a wrapped file: it must be an array.
struct Conversations<'a> {
    marks: &'a Marks,
}

impl<'de> DeserializeSeed<'de> for Conversations<'_> {
    type Value = Vec<Kept>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Conversations<'_> {
    type Value = Vec<Kept>;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.marks.wrong_shape.set(true);
        write!(f, "an array of conversations")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        keep_each(seq, true, self.marks)
    }
}

/// Parses each conversation of the sequence and keeps it at once.
fn keep_each<'de, A: SeqAccess<'de>>(
    mut seq: A,
    already_processed: bool,
    marks: &Marks,
) -> Result<Vec<Kept>, A::Error> {
    let mut kept = Vec::new();
    while let Some(conversation) = seq.next_element::<Conversation>()? {
        match keep_conversation(&conversation, already_processed) {
            Ok(k) => kept.push(k),
            Err(e) => {
                let message = e.to_string();
                marks.failure.set(Some(e));
                return Err(serde::de::Error::custom(message));
            }
        }
    }
    Ok(kept)
}
