// The Describe page's rules and conversions (core/conversation-metadata.js;
// plan docs/plans/2026-10-05-screen-flow.md §7). Run with TZ=UTC (see
// package.json), so local date-and-time inputs are UTC here.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  MEDIUM_KINDS, NAME_CAP, PARTICIPANT_KINDS, TRANSCRIPTION_SERVICES, checkAnswers, describeMedium,
  describeOrigin, describeParticipants, isoToLocalInput, localInputToIso, mediumToAnswer,
  participantsToAnswers, spanToAnswer,
} from '../core/conversation-metadata.js';

const human = (name) => ({ kind: 'human', name });

test('the choices match the server\'s names', () => {
  assert.deepEqual(PARTICIPANT_KINDS.map((k) => k.value), ['human', 'claude', 'chatgpt', 'gemini', 'other_ai']);
  assert.deepEqual(MEDIUM_KINDS.map((k) => k.value), ['typed', 'virtual_voice', 'live_voice']);
  assert.equal(TRANSCRIPTION_SERVICES.length, 13);
  assert.equal(TRANSCRIPTION_SERVICES.at(-1).value, 'other');
});

test('server metadata becomes answers, and "varies" stays null', () => {
  assert.deepEqual(participantsToAnswers([{ kind: 'human', name: 'Ada' }, { kind: 'claude' }]),
    [human('Ada'), { kind: 'claude', name: '' }]);
  assert.equal(participantsToAnswers(null), null);
  assert.deepEqual(mediumToAnswer({ kind: 'typed' }), { kind: 'typed', service: '', serviceName: '' });
  assert.deepEqual(mediumToAnswer({ kind: 'live_voice', transcription: { service: 'other', name: 'Dictaphone' } }),
    { kind: 'live_voice', service: 'other', serviceName: 'Dictaphone' });
  assert.equal(mediumToAnswer(null), null);
});

test('dates and times go to the inputs and back with their offset', () => {
  assert.equal(isoToLocalInput('2026-01-02T03:04:00Z'), '2026-01-02T03:04');
  assert.equal(localInputToIso('2026-01-02T03:04'), '2026-01-02T03:04:00+00:00');
  assert.equal(localInputToIso(''), null);
  assert.equal(localInputToIso('2026-13-40T99:99'), null);
  assert.deepEqual(spanToAnswer({ start: '2026-01-02T03:04:00Z', end: '2026-01-02T05:00:00Z' }),
    { start: '2026-01-02T03:04', end: '2026-01-02T05:00' });
});

test('good answers become the edit, cleaned', () => {
  const { errors, edit } = checkAnswers({
    participants: [human('  Ada​ '), { kind: 'claude', name: 'ignored' }, { kind: 'other_ai', name: 'Le Chat' }],
    medium: { kind: 'virtual_voice', service: 'zoom', serviceName: '' },
    span: { start: '2026-01-01T10:00', end: '2026-01-01T11:00' },
  });
  assert.deepEqual(errors, {});
  assert.deepEqual(edit, {
    participants: [human('Ada'), { kind: 'claude' }, { kind: 'other_ai', name: 'Le Chat' }],
    medium: { kind: 'virtual_voice', transcription: { service: 'zoom' } },
    span: { start: '2026-01-01T10:00:00+00:00', end: '2026-01-01T11:00:00+00:00' },
  });
});

test('a varies answer is left out of the edit', () => {
  const { edit } = checkAnswers({ participants: null, medium: { kind: 'typed' }, span: null });
  assert.deepEqual(edit, { medium: { kind: 'typed' } });
});

test('another service needs its name; a named service does not', () => {
  assert.deepEqual(checkAnswers({ medium: { kind: 'live_voice', service: 'other', serviceName: ' Tactiq ' } }).edit,
    { medium: { kind: 'live_voice', transcription: { service: 'other', name: 'Tactiq' } } });
  assert.deepEqual(checkAnswers({ medium: { kind: 'live_voice', service: 'other', serviceName: '' } }).errors,
    { serviceName: 'Name the transcription service.' });
});

test('every rule refuses its broken answer and says which field', () => {
  const cases = [
    [{ participants: [] }, 'participants'],
    [{ participants: [{ kind: 'robot', name: '' }] }, 'participant.0'],
    [{ participants: [human(' ')] }, 'participant.0'],
    [{ participants: [{ kind: 'other_ai', name: '' }] }, 'participant.0'],
    [{ medium: { kind: 'telepathy' } }, 'medium'],
    [{ medium: { kind: 'virtual_voice', service: '' } }, 'service'],
    [{ span: { start: '', end: '2026-01-01T11:00' } }, 'span'],
    [{ span: { start: '2026-01-01T12:00', end: '2026-01-01T11:00' } }, 'span'],
  ];
  for(const [answers, field] of cases){
    const { errors, edit } = checkAnswers(answers);
    assert.equal(edit, null, JSON.stringify(answers));
    assert.ok(errors[field], `${JSON.stringify(answers)} -> ${JSON.stringify(errors)}`);
  }
});

test('a long name is cut to the cap', () => {
  const { edit } = checkAnswers({ participants: [human('x'.repeat(NAME_CAP + 10))] });
  assert.equal(edit.participants[0].name.length, NAME_CAP);
});

test('short descriptions for the Files and Conversations tabs', () => {
  assert.equal(describeParticipants([{ kind: 'human', name: 'Ada' }, { kind: 'claude' }]), 'Ada, Claude');
  assert.equal(describeParticipants(null), 'varies');
  assert.equal(describeMedium({ kind: 'typed' }), 'Typed');
  assert.equal(describeMedium({ kind: 'virtual_voice', transcription: { service: 'google_meet' } }), 'Virtual voice (Google Meet)');
  assert.equal(describeMedium({ kind: 'live_voice', transcription: { service: 'other', name: 'Tactiq' } }), 'Live voice (Tactiq)');
  assert.equal(describeMedium({ kind: 'mystery' }), 'mystery');
  assert.equal(describeMedium(null), 'varies');
  assert.equal(describeOrigin('confirmed'), 'confirmed');
  assert.equal(describeOrigin('guessed'), 'guessed');
  assert.equal(describeOrigin(null), 'some guessed');
});

test('west of UTC the offset is negative', () => {
  const saved = process.env.TZ;
  process.env.TZ = 'America/New_York';
  try{
    assert.equal(localInputToIso('2026-01-02T03:04'), '2026-01-02T03:04:00-05:00');
    assert.equal(isoToLocalInput('2026-01-02T08:04:00Z'), '2026-01-02T03:04');
  } finally {
    process.env.TZ = saved;
  }
});
