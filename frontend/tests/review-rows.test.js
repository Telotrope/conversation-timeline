// Review's rows read by the page, and the wording of a pruned branch's
// note (core/review-rows.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4d, the user's
// wording of 2026-10-06).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { messageOf, noteWording, overridesOf } from '../core/review-rows.js';

const row = (auto, user) => ({
  kind: 'message', conversation_id: 'c', message_id: 'm', at: '2026-01-01T10:00:00Z', handle: 'h',
  flags: { auto, user }, pieces: [], attachments: [],
});

test('a row becomes the message the flag rules read, with what a save needs', () => {
  assert.deepEqual(messageOf(row({ caps: true, critical: false, angry: true }, { caps: null, critical: null, angry: null })), {
    id: 'm', conversationId: 'c', messageId: 'm', handle: 'h', at: '2026-01-01T10:00:00Z',
    default_caps: true, default_critical: false, default_angry: true, auto_source: 'heuristic',
  });
  const unscanned = messageOf(row(null, { caps: null, critical: null, angry: null }));
  assert.deepEqual([unscanned.default_caps, unscanned.auto_source], [false, 'none']);
});

test('only the flags you stated are your corrections', () => {
  assert.equal(overridesOf(row(null, { caps: null, critical: null, angry: null })), null);
  assert.deepEqual(overridesOf(row(null, { caps: false, critical: null, angry: true })), { caps: false, angry: true });
});

const clock = (iso) => iso.slice(11, 16);
const note = (extra) => ({
  key: { conversation_id: 'c', at: '2026-06-21T16:30:00Z', id: 'n' }, last_at: '2026-06-21T16:31:00Z',
  messages: 2, words_not_repeated: 155, replaced_by: 'm2', kept_as: null, ...extra,
});
const text = (w) => w.lead + (w.link || '') + w.rest;

test('a pruned branch says how much was lost and when', () => {
  assert.equal(text(noteWording(note({}), clock)),
    'An earlier branch of this conversation was pruned here: 2 messages (155 words not repeated below) from 16:30 to 16:31, replaced by the message below.');
  assert.equal(text(noteWording(note({ messages: 1, words_not_repeated: 1, replaced_by: null }), clock)),
    'An earlier branch of this conversation was pruned here: 1 message (1 word not repeated below) from 16:30 to 16:31.');
});

test('a branch that only repeated the message below is an earlier copy', () => {
  const w = noteWording(note({ words_not_repeated: 0 }), clock);
  assert.deepEqual(w, { lead: 'An earlier copy of the message below was pruned here (sent 16:30).', link: null, rest: '' });
  assert.equal(text(noteWording(note({ words_not_repeated: 0, key: { at: '1970-01-01T00:00:00Z' } }), clock)),
    'An earlier copy of the message below was pruned here (sent an unknown time).');
});

test('a branch kept as its own conversation links to it', () => {
  const w = noteWording(note({ kept_as: 'c2' }), clock);
  assert.equal(w.link, 'its own conversation');
  assert.equal(text(w),
    'An earlier branch of this conversation was kept as its own conversation: 2 messages (155 words not repeated below) from 16:30 to 16:31, replaced by the message below.');
});
