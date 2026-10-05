//! Conversation metadata (plan docs/plans/2026-10-05-screen-flow.md §7a,
//! §7b, §8a): the rule-enforcing types, the guess made at upload, and edits.

use chrono::{DateTime, Duration, FixedOffset, Utc};
use serde_json::json;
use timeline_core::conversation_metadata::{
    guess_summary, message_span, ConversationMedium, ConversationSpan, MetadataEdit, MetadataError,
    MetadataOrigin, Participant, Participants, TranscriptionService, UploadFacts,
    UNDATED_LENGTH_HOURS,
};
use timeline_core::labels::{AiName, FileName, PersonName, ServiceName};
use timeline_core::ports::ids::UploadId;
use timeline_core::{unwrap_uploaded_json, Conversation};

fn at(text: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(text).unwrap()
}

fn utc(text: &str) -> DateTime<Utc> {
    at(text).with_timezone(&Utc)
}

fn conversation(times: &[&str]) -> Conversation {
    let messages: Vec<_> = times
        .iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "uuid": format!("00000000-0000-4000-8000-{:012}", i + 1),
                "sender": if i % 2 == 0 { "human" } else { "assistant" },
                "created_at": t,
                "content": [{"type": "text", "text": format!("message {i}")}],
            })
        })
        .collect();
    let raw = json!([{
        "uuid": "11111111-1111-4111-8111-111111111111",
        "name": "A chat",
        "chat_messages": messages,
    }]);
    unwrap_uploaded_json(&raw.to_string())
        .unwrap()
        .conversations
        .remove(0)
}

fn facts(written: Option<&str>) -> UploadFacts {
    UploadFacts {
        file_name: FileName::parse("conversations.json").unwrap(),
        uploaded_at: utc("2026-10-05T12:00:00Z"),
        file_written_at: written.map(utc),
        human_name: PersonName::parse("ada@example.com").unwrap(),
    }
}

fn upload() -> UploadId {
    UploadId(uuid::Uuid::from_u128(9))
}

#[test]
fn a_participant_list_cannot_be_empty() {
    assert_eq!(
        Participants::new(vec![]),
        Err(MetadataError::NoParticipants)
    );
    let err = serde_json::from_value::<Participants>(json!([])).unwrap_err();
    assert!(
        err.to_string().contains("at least one participant"),
        "{err}"
    );
    let one = Participants::new(vec![Participant::Gemini]).unwrap();
    assert_eq!(one.as_slice(), &[Participant::Gemini]);
}

#[test]
fn participants_read_and_write_as_tagged_json() {
    let list = Participants::new(vec![
        Participant::Human {
            name: PersonName::parse("Ada").unwrap(),
        },
        Participant::Claude,
        Participant::ChatGpt,
        Participant::Gemini,
        Participant::OtherAi {
            name: AiName::parse("Le Chat").unwrap(),
        },
    ])
    .unwrap();
    let as_json = json!([
        {"kind": "human", "name": "Ada"},
        {"kind": "claude"},
        {"kind": "chatgpt"},
        {"kind": "gemini"},
        {"kind": "other_ai", "name": "Le Chat"},
    ]);
    assert_eq!(serde_json::to_value(&list).unwrap(), as_json);
    assert_eq!(
        serde_json::from_value::<Participants>(as_json).unwrap(),
        list
    );
}

#[test]
fn a_human_or_other_ai_needs_a_name() {
    assert!(serde_json::from_value::<Participant>(json!({"kind": "human"})).is_err());
    assert!(serde_json::from_value::<Participant>(json!({"kind": "human", "name": " "})).is_err());
    assert!(serde_json::from_value::<Participant>(json!({"kind": "other_ai"})).is_err());
}

#[test]
fn only_the_voice_kinds_carry_a_transcription_service() {
    let typed = serde_json::from_value::<ConversationMedium>(json!({"kind": "typed"})).unwrap();
    assert_eq!(typed, ConversationMedium::Typed);
    assert!(
        serde_json::from_value::<ConversationMedium>(json!({"kind": "virtual_voice"})).is_err()
    );
    let meeting = ConversationMedium::VirtualVoice {
        transcription: TranscriptionService::Zoom,
    };
    assert_eq!(
        serde_json::to_value(&meeting).unwrap(),
        json!({"kind": "virtual_voice", "transcription": {"service": "zoom"}})
    );
    let live = ConversationMedium::LiveVoice {
        transcription: TranscriptionService::Other {
            name: ServiceName::parse("A dictaphone app").unwrap(),
        },
    };
    let live_json = json!({"kind": "live_voice", "transcription": {"service": "other", "name": "A dictaphone app"}});
    assert_eq!(serde_json::to_value(&live).unwrap(), live_json);
    assert_eq!(
        serde_json::from_value::<ConversationMedium>(live_json).unwrap(),
        live
    );
}

#[test]
fn every_named_transcription_service_round_trips() {
    for name in [
        "zoom",
        "google_meet",
        "microsoft_teams",
        "webex",
        "skype",
        "otter_ai",
        "fireflies",
        "rev",
        "whisper",
        "phone_recorder",
        "person",
        "unknown",
    ] {
        let value = json!({"service": name});
        let service: TranscriptionService = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&service).unwrap(), value);
    }
}

#[test]
fn a_span_cannot_end_before_it_starts() {
    let start = at("2026-01-01T10:00:00+02:00");
    assert_eq!(
        ConversationSpan::new(start, start - Duration::seconds(1)),
        Err(MetadataError::EndBeforeStart)
    );
    assert_eq!(
        MetadataError::EndBeforeStart.to_string(),
        "a conversation can't end before it starts"
    );
    assert!(MetadataError::NoParticipants
        .to_string()
        .contains("participant"));
    let err = serde_json::from_value::<ConversationSpan>(
        json!({"start": "2026-01-01T10:00:00Z", "end": "2026-01-01T09:00:00Z"}),
    )
    .unwrap_err();
    assert!(err.to_string().contains("can't end before"), "{err}");
}

#[test]
fn a_span_keeps_its_offsets_and_widens_to_cover_another() {
    let span = ConversationSpan::new(
        at("2026-01-01T10:00:00+02:00"),
        at("2026-01-01T11:00:00+02:00"),
    )
    .unwrap();
    let json = serde_json::to_value(span).unwrap();
    assert_eq!(
        json,
        json!({"start": "2026-01-01T10:00:00+02:00", "end": "2026-01-01T11:00:00+02:00"})
    );
    assert_eq!(
        serde_json::from_value::<ConversationSpan>(json).unwrap(),
        span
    );

    let later =
        ConversationSpan::new(at("2026-01-02T10:00:00Z"), at("2026-01-02T12:00:00Z")).unwrap();
    let wide = span.widened_to(&later);
    assert_eq!(wide.start(), span.start());
    assert_eq!(wide.end(), later.end());
}

#[test]
fn the_message_span_runs_from_the_earliest_to_the_latest_message() {
    let c = conversation(&[
        "2026-01-01T10:05:00Z",
        "2026-01-01T10:00:00Z",
        "2026-01-01T10:30:00Z",
    ]);
    let span = message_span(&c).unwrap();
    assert_eq!(span.start(), at("2026-01-01T10:00:00Z"));
    assert_eq!(span.end(), at("2026-01-01T10:30:00Z"));
    assert_eq!(message_span(&conversation(&[])), None);
}

#[test]
fn the_guess_names_the_signed_in_human_and_claude_and_says_typed() {
    let c = conversation(&["2026-01-01T10:00:00Z", "2026-01-01T10:01:00Z"]);
    let s = guess_summary(&c, upload(), &facts(None));
    assert_eq!(s.conversation_id, c.uuid);
    assert_eq!(s.name, c.name);
    assert_eq!(s.source.upload_id, upload());
    assert_eq!(s.source.file_name.as_str(), "conversations.json");
    assert_eq!(s.source.uploaded_at, utc("2026-10-05T12:00:00Z"));
    assert_eq!(s.source.file_written_at, None);
    assert!(s.additions.is_empty());
    assert_eq!(s.message_count, 2);
    assert_eq!(
        s.participants.as_slice(),
        &[
            Participant::Human {
                name: PersonName::parse("ada@example.com").unwrap()
            },
            Participant::Claude
        ]
    );
    assert_eq!(s.medium, ConversationMedium::Typed);
    assert_eq!(s.details_origin, MetadataOrigin::Guessed);
    assert_eq!(s.span_origin, MetadataOrigin::Guessed);
    assert_eq!(s.span.start(), at("2026-01-01T10:00:00Z"));
    assert_eq!(s.span.end(), at("2026-01-01T10:01:00Z"));
    assert_eq!(s.message_span, Some(s.span));
}

#[test]
fn with_no_message_times_the_guess_is_the_hour_before_the_file_was_written() {
    let s = guess_summary(
        &conversation(&[]),
        upload(),
        &facts(Some("2026-09-30T08:00:00Z")),
    );
    assert_eq!(s.message_span, None);
    assert_eq!(s.message_count, 0);
    assert_eq!(s.span.end(), at("2026-09-30T08:00:00Z"));
    assert_eq!(
        s.span.start(),
        at("2026-09-30T08:00:00Z") - Duration::hours(UNDATED_LENGTH_HOURS)
    );
    assert_eq!(s.source.file_written_at, Some(utc("2026-09-30T08:00:00Z")));
}

#[test]
fn with_no_message_times_or_written_time_the_guess_is_the_hour_before_upload() {
    let s = guess_summary(&conversation(&[]), upload(), &facts(None));
    assert_eq!(s.span.end(), at("2026-10-05T12:00:00Z"));
    assert_eq!(s.span.start(), at("2026-10-05T11:00:00Z"));
}

#[test]
fn an_edit_changes_only_the_fields_it_carries_and_marks_them_confirmed() {
    let base = guess_summary(
        &conversation(&["2026-01-01T10:00:00Z"]),
        upload(),
        &facts(None),
    );

    let mut s = base.clone();
    MetadataEdit::default().apply_to(&mut s);
    assert_eq!(s, base);

    let medium = ConversationMedium::LiveVoice {
        transcription: TranscriptionService::Person,
    };
    let mut s = base.clone();
    MetadataEdit {
        medium: Some(medium.clone()),
        ..Default::default()
    }
    .apply_to(&mut s);
    assert_eq!(s.medium, medium);
    assert_eq!(s.participants, base.participants);
    assert_eq!(s.details_origin, MetadataOrigin::Confirmed);
    assert_eq!(s.span_origin, MetadataOrigin::Guessed);

    let people = Participants::new(vec![Participant::ChatGpt]).unwrap();
    let span =
        ConversationSpan::new(at("2025-01-01T00:00:00Z"), at("2025-01-01T02:00:00Z")).unwrap();
    let mut s = base.clone();
    MetadataEdit {
        participants: Some(people.clone()),
        span: Some(span),
        ..Default::default()
    }
    .apply_to(&mut s);
    assert_eq!(s.participants, people);
    assert_eq!(s.medium, base.medium);
    assert_eq!(s.span, span);
    assert_eq!(s.details_origin, MetadataOrigin::Confirmed);
    assert_eq!(s.span_origin, MetadataOrigin::Confirmed);
}

#[test]
fn an_edit_reads_from_json_with_every_field_optional_and_no_strangers() {
    let edit: MetadataEdit = serde_json::from_value(json!({"medium": {"kind": "typed"}})).unwrap();
    assert_eq!(edit.medium, Some(ConversationMedium::Typed));
    assert_eq!(edit.participants, None);
    assert_eq!(edit.span, None);
    assert_eq!(
        serde_json::from_value::<MetadataEdit>(json!({})).unwrap(),
        MetadataEdit::default()
    );
    assert!(serde_json::from_value::<MetadataEdit>(json!({"colour": "red"})).is_err());
}

#[test]
fn origins_and_upload_facts_round_trip() {
    assert_eq!(
        serde_json::to_value(MetadataOrigin::Confirmed).unwrap(),
        json!("confirmed")
    );
    let f = facts(Some("2026-09-30T08:00:00Z"));
    let back: UploadFacts = serde_json::from_value(serde_json::to_value(&f).unwrap()).unwrap();
    assert_eq!(back, f);
}
