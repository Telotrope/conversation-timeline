// A conversation's metadata as the Describe page edits it (plan
// docs/plans/2026-10-05-screen-flow.md §7): the choices it offers, the
// server's metadata turned into form answers and back, the rules an answer
// must follow, and short descriptions for the Files tab and the
// Conversations tab. No page access.
//
// The server checks the same rules again (timeline-core's
// conversation_metadata.rs); checking here too lets the page mark the field
// that is wrong before anything is sent.

import { cleanText } from './activity-event.js';

// The most characters a typed name keeps, matching the server's LABEL_CAP.
export const NAME_CAP = 200;

export const PARTICIPANT_KINDS = Object.freeze([
  { value: 'human', label: 'Human', named: true },
  { value: 'claude', label: 'Claude', named: false },
  { value: 'chatgpt', label: 'ChatGPT', named: false },
  { value: 'gemini', label: 'Gemini', named: false },
  { value: 'other_ai', label: 'Other AI', named: true },
]);

export const MEDIUM_KINDS = Object.freeze([
  { value: 'typed', label: 'Typed', voice: false,
    explain: 'Typed messages, as in a chat.' },
  { value: 'virtual_voice', label: 'Virtual voice', voice: true,
    explain: 'Spoken in an online meeting room such as Zoom or Meet, each person recorded by the microphone on their own computer.' },
  { value: 'live_voice', label: 'Live voice', voice: true,
    explain: 'Spoken in a shared physical space, recorded by one microphone or written down by a person.' },
]);

export const TRANSCRIPTION_SERVICES = Object.freeze([
  { value: 'zoom', label: 'Zoom' },
  { value: 'google_meet', label: 'Google Meet' },
  { value: 'microsoft_teams', label: 'Microsoft Teams' },
  { value: 'webex', label: 'Webex' },
  { value: 'skype', label: 'Skype' },
  { value: 'otter_ai', label: 'Otter.ai' },
  { value: 'fireflies', label: 'Fireflies.ai' },
  { value: 'rev', label: 'Rev' },
  { value: 'whisper', label: 'Whisper' },
  { value: 'phone_recorder', label: "A phone's built-in recorder" },
  { value: 'person', label: 'A person' },
  { value: 'unknown', label: "Don't know" },
  { value: 'other', label: 'Other' },
]);

function labelOf(list, value){
  const found = list.find((x) => x.value === value);
  return found ? found.label : value;
}

// --- Server metadata to form answers ---
// A form's answers: { participants, medium, span }. `participants` is a list
// of { kind, name } and `medium` is { kind, service, serviceName }; either
// is null when the conversations being described disagree ("varies"), and
// stays null until the user chooses something. `span` is { start, end } as
// the browser's date-and-time inputs hold them, or null when not edited
// here (a file's span differs by conversation).

export function participantsToAnswers(list){
  if(!list) return null;
  return list.map((p) => ({ kind: p.kind, name: p.name || '' }));
}

export function mediumToAnswer(medium){
  if(!medium) return null;
  const t = medium.transcription || null;
  return {
    kind: medium.kind,
    service: t ? t.service : '',
    serviceName: t && t.name ? t.name : '',
  };
}

// An ISO date and time as a date-and-time input's value, in the browser's
// own time zone ('YYYY-MM-DDTHH:MM').
export function isoToLocalInput(iso){
  const d = new Date(iso);
  const pad = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

// A date-and-time input's value as an ISO date and time carrying the
// browser's offset from UTC at that moment, or null when it isn't one.
export function localInputToIso(value){
  if(!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value || '')) return null;
  const d = new Date(value);
  if(Number.isNaN(d.getTime())) return null;
  const offset = -d.getTimezoneOffset();
  const sign = offset >= 0 ? '+' : '-';
  const abs = Math.abs(offset);
  const pad = (n) => String(n).padStart(2, '0');
  return `${value}:00${sign}${pad(Math.floor(abs / 60))}:${pad(abs % 60)}`;
}

export function spanToAnswer(span){
  return { start: isoToLocalInput(span.start), end: isoToLocalInput(span.end) };
}

// --- Form answers to the server's edit, checked ---
// Resolves the answers to { errors, edit }. `errors` maps a field
// ('participants', 'participant.<i>', 'medium', 'service', 'serviceName',
// 'span') to what's wrong; `edit` is the body for the metadata routes,
// holding only the fields the answers decide (a null "varies" answer is
// left out), and is null when there are errors.
export function checkAnswers(answers){
  const errors = {};
  const edit = {};
  if(answers.participants){
    if(answers.participants.length === 0) errors.participants = 'Add at least one participant.';
    edit.participants = answers.participants.map((p, i) => {
      const kind = PARTICIPANT_KINDS.find((k) => k.value === p.kind);
      if(!kind){
        errors[`participant.${i}`] = 'Choose who this is.';
        return null;
      }
      if(!kind.named) return { kind: p.kind };
      const name = cleanText(p.name, NAME_CAP);
      if(!name) errors[`participant.${i}`] = 'Give a name.';
      return { kind: p.kind, name };
    });
  }
  if(answers.medium){
    edit.medium = checkMedium(answers.medium, errors);
  }
  if(answers.span){
    edit.span = checkSpan(answers.span, errors);
  }
  return Object.keys(errors).length ? { errors, edit: null } : { errors, edit };
}

function checkMedium(answer, errors){
  const kind = MEDIUM_KINDS.find((k) => k.value === answer.kind);
  if(!kind){
    errors.medium = 'Choose how the conversation was held.';
    return null;
  }
  if(!kind.voice) return { kind: answer.kind };
  if(!TRANSCRIPTION_SERVICES.some((s) => s.value === answer.service)){
    errors.service = 'Choose who transcribed it.';
    return null;
  }
  if(answer.service !== 'other') return { kind: answer.kind, transcription: { service: answer.service } };
  const name = cleanText(answer.serviceName, NAME_CAP);
  if(!name){
    errors.serviceName = 'Name the transcription service.';
    return null;
  }
  return { kind: answer.kind, transcription: { service: 'other', name } };
}

function checkSpan(span, errors){
  const start = localInputToIso(span.start);
  const end = localInputToIso(span.end);
  if(!start || !end){
    errors.span = 'Give both a start and an end.';
    return null;
  }
  if(new Date(end) < new Date(start)){
    errors.span = "The end can't be before the start.";
    return null;
  }
  return { start, end };
}

// --- Short descriptions ---

export function describeParticipants(list){
  if(!list) return 'varies';
  return list.map((p) => p.name || labelOf(PARTICIPANT_KINDS, p.kind)).join(', ');
}

export function describeMedium(medium){
  if(!medium) return 'varies';
  const kind = labelOf(MEDIUM_KINDS, medium.kind);
  const t = medium.transcription;
  if(!t) return kind;
  return `${kind} (${t.service === 'other' ? t.name : labelOf(TRANSCRIPTION_SERVICES, t.service)})`;
}

export function describeOrigin(origin){
  if(!origin) return 'some guessed';
  return origin === 'confirmed' ? 'confirmed' : 'guessed';
}
