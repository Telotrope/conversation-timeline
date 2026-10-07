//! What a conversation's messages don't say about it: who took part, how it
//! was held, how it was transcribed, when it started and ended, and which
//! file it came from (plan `docs/plans/2026-10-05-screen-flow.md` §7a, §8a).
//! These are fields of [`ConversationSummary`], the one record kept per
//! conversation, not a second record beside it.
//!
//! The types make a broken answer unrepresentable rather than checking it
//! wherever it's read: a participant list can't be empty, a typed
//! conversation can't have a transcription service, and a span can't end
//! before it starts.

use std::fmt;

use chrono::{DateTime, Duration, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

use crate::labels::{AiName, FileName, PersonName, ServiceName};
use crate::message_time::MessageTime;
use crate::model::Conversation;
use crate::ports::conversations::ConversationSummary;
use crate::ports::ids::UploadId;

/// One participant in a conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Participant {
    Human {
        name: PersonName,
    },
    Claude,
    #[serde(rename = "chatgpt")]
    ChatGpt,
    Gemini,
    OtherAi {
        name: AiName,
    },
}

/// A conversation's participants: never empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Participant>", into = "Vec<Participant>")]
pub struct Participants(Vec<Participant>);

/// Why a set of answers was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetadataError {
    NoParticipants,
    EndBeforeStart,
}

impl fmt::Display for MetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetadataError::NoParticipants => {
                write!(f, "a conversation needs at least one participant")
            }
            MetadataError::EndBeforeStart => write!(f, "a conversation can't end before it starts"),
        }
    }
}

impl std::error::Error for MetadataError {}

impl Participants {
    pub fn new(list: Vec<Participant>) -> Result<Self, MetadataError> {
        if list.is_empty() {
            return Err(MetadataError::NoParticipants);
        }
        Ok(Self(list))
    }

    pub fn as_slice(&self) -> &[Participant] {
        &self.0
    }
}

impl TryFrom<Vec<Participant>> for Participants {
    type Error = MetadataError;
    fn try_from(list: Vec<Participant>) -> Result<Self, MetadataError> {
        Self::new(list)
    }
}

impl From<Participants> for Vec<Participant> {
    fn from(p: Participants) -> Self {
        p.0
    }
}

/// Who or what turned a voice conversation into text. Each named service is
/// kept by name because later guessing and error analysis treat them
/// differently; anything else is `Other`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum TranscriptionService {
    Zoom,
    GoogleMeet,
    MicrosoftTeams,
    Webex,
    Skype,
    OtterAi,
    Fireflies,
    Rev,
    Whisper,
    PhoneRecorder,
    Person,
    Unknown,
    Other { name: ServiceName },
}

/// How a conversation was held. Only the two voice kinds have a
/// transcription service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConversationMedium {
    Typed,
    /// In an online meeting room (Zoom, Meet, ...), each person recorded by
    /// their own computer's microphone.
    VirtualVoice {
        transcription: TranscriptionService,
    },
    /// In a shared physical space, recorded by one microphone or written
    /// down by a person.
    LiveVoice {
        transcription: TranscriptionService,
    },
}

/// When a conversation started and ended. Never ends before it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawSpan", into = "RawSpan")]
pub struct ConversationSpan {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
}

#[derive(Serialize, Deserialize)]
struct RawSpan {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
}

impl ConversationSpan {
    pub fn new(
        start: DateTime<FixedOffset>,
        end: DateTime<FixedOffset>,
    ) -> Result<Self, MetadataError> {
        if end < start {
            return Err(MetadataError::EndBeforeStart);
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> DateTime<FixedOffset> {
        self.start
    }

    pub fn end(&self) -> DateTime<FixedOffset> {
        self.end
    }

    /// The smallest span covering both.
    pub fn widened_to(&self, other: &ConversationSpan) -> ConversationSpan {
        ConversationSpan {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl TryFrom<RawSpan> for ConversationSpan {
    type Error = MetadataError;
    fn try_from(raw: RawSpan) -> Result<Self, MetadataError> {
        Self::new(raw.start, raw.end)
    }
}

impl From<ConversationSpan> for RawSpan {
    fn from(span: ConversationSpan) -> Self {
        RawSpan {
            start: span.start,
            end: span.end,
        }
    }
}

/// Whether a value is still the upload's guess or was saved by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataOrigin {
    Guessed,
    Confirmed,
}

/// The first file a conversation came in. Never changed after upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub upload_id: UploadId,
    pub file_name: FileName,
    pub uploaded_at: DateTime<Utc>,
    /// When the file was last written, as the browser reported it; `None`
    /// when it didn't.
    pub file_written_at: Option<DateTime<Utc>>,
}

/// What an upload knows that its file doesn't: recorded by `POST /uploads`
/// and read back by processing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UploadFacts {
    pub file_name: FileName,
    pub uploaded_at: DateTime<Utc>,
    pub file_written_at: Option<DateTime<Utc>>,
    /// The name the guess gives the one human: the signed-in account.
    pub human_name: PersonName,
}

/// How long a conversation with no times of its own is guessed to have
/// lasted, so none is zero length (the user, 2026-10-05).
pub const UNDATED_LENGTH_HOURS: i64 = 1;

/// The span of a conversation's known message times, or `None` when no
/// message has a time.
pub fn message_span(conversation: &Conversation) -> Option<ConversationSpan> {
    span_of_times(
        conversation
            .chat_messages
            .iter()
            .filter_map(|m| m.time().known()),
    )
}

/// From the earliest to the latest of `times`, or `None` when there are
/// none.
pub fn span_of_times(mut times: impl Iterator<Item = DateTime<Utc>>) -> Option<ConversationSpan> {
    let first = times.next()?;
    let (start, end) = times.fold((first, first), |(s, e), t| (s.min(t), e.max(t)));
    Some(ConversationSpan {
        start: start.fixed_offset(),
        end: end.fixed_offset(),
    })
}

/// A new conversation's record, with the guessed fields filled in (plan
/// §7b). The one place guesses are made; the later guessing plan replaces
/// the guesses here.
///
/// - Participants: one human named with the signed-in account, and Claude
///   (the only export format read today is Claude's).
/// - Typed, so no transcription service.
/// - Start and end: the earliest and latest known message times; with none,
///   an hour ending when the file was last written, or else when it was
///   uploaded.
pub fn guess_summary(
    conversation: &Conversation,
    upload_id: UploadId,
    facts: &UploadFacts,
) -> ConversationSummary {
    let messages = message_span(conversation);
    let span = messages.unwrap_or_else(|| {
        let end = facts.file_written_at.unwrap_or(facts.uploaded_at);
        ConversationSpan {
            start: (end - Duration::hours(UNDATED_LENGTH_HOURS)).fixed_offset(),
            end: end.fixed_offset(),
        }
    });
    ConversationSummary {
        conversation_id: conversation.uuid,
        name: conversation.name.clone(),
        version: 0,
        source: SourceFile {
            upload_id,
            file_name: facts.file_name.clone(),
            uploaded_at: facts.uploaded_at,
            file_written_at: facts.file_written_at,
        },
        additions: Vec::new(),
        message_count: conversation.chat_messages.len(),
        untimed: conversation
            .chat_messages
            .iter()
            .filter(|m| m.time() == MessageTime::Unknown)
            .count(),
        // Counted from the stored rows, in the file's order, by whoever
        // writes the record (plan §12.3).
        out_of_order: 0,
        message_span: messages,
        participants: Participants(vec![
            Participant::Human {
                name: facts.human_name.clone(),
            },
            Participant::Claude,
        ]),
        medium: ConversationMedium::Typed,
        details_origin: MetadataOrigin::Guessed,
        span,
        span_origin: MetadataOrigin::Guessed,
        branch_of: None,
        branches: Vec::new(),
    }
}

/// A change to a conversation's metadata. Every field is optional: `None`
/// leaves that field as it is, which is what lets a file-wide save change
/// only the fields the user changed. Each field given is marked confirmed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetadataEdit {
    #[serde(default)]
    pub participants: Option<Participants>,
    #[serde(default)]
    pub medium: Option<ConversationMedium>,
    #[serde(default)]
    pub span: Option<ConversationSpan>,
}

impl MetadataEdit {
    /// Applies this edit to `summary`.
    pub fn apply_to(&self, summary: &mut ConversationSummary) {
        if let Some(participants) = &self.participants {
            summary.participants = participants.clone();
            summary.details_origin = MetadataOrigin::Confirmed;
        }
        if let Some(medium) = &self.medium {
            summary.medium = medium.clone();
            summary.details_origin = MetadataOrigin::Confirmed;
        }
        if let Some(span) = self.span {
            summary.span = span;
            summary.span_origin = MetadataOrigin::Confirmed;
        }
    }
}
