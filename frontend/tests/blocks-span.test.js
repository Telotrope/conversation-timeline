// Conversations whose messages have no times are placed by their start and
// end (core/blocks.js; plan docs/plans/2026-10-05-screen-flow.md §7f, C15).
// No file the server reads today has untimed messages, so this is shown
// here, by handing buildBlocks such conversations directly.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import { buildBlocks } from '../core/blocks.js';
import { resetState } from './fixtures.js';

beforeEach(() => {
  resetState();
  state.records = new Map([
    ['c0', { span: { start: '2026-02-01T09:00:00+01:00', end: '2026-02-01T10:30:00+01:00' } }],
    ['c1', { span: { start: '2026-02-02T09:00:00Z', end: '2026-02-02T09:30:00Z' } }],
  ]);
});

test('a conversation with no message times is one session from its start to its end', () => {
  state.conversations = [{ name: 'voice', total_messages: 3, id: 'c0', untimed: 3 }];
  state.messages = [];
  assert.deepEqual(buildBlocks(), [{
    conv: 0, date: '2026-02-01', start: '2026-02-01T08:00:00.000Z', end: '2026-02-01T09:30:00.000Z',
    duration_sec: 5400, count: 3,
  }]);
});

test('editing its start and end moves it', () => {
  state.conversations = [{ name: 'voice', total_messages: 3, id: 'c0', untimed: 3 }];
  state.records.get('c0').span = { start: '2026-03-05T12:00:00Z', end: '2026-03-05T13:00:00Z' };
  assert.equal(buildBlocks()[0].start, '2026-03-05T12:00:00.000Z');
});

test('some timed messages and some not: placed by start and end, not by the timed ones', () => {
  state.conversations = [{ name: 'mixed', total_messages: 4, id: 'c1', untimed: 1 }];
  state.messages = [{ conv: 0, ts: '2026-05-05T10:00:00Z' }, { conv: 0, ts: '2026-05-05T10:01:00Z' }];
  const blocks = buildBlocks();
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].start, '2026-02-02T09:00:00.000Z');
  assert.equal(blocks[0].count, 4);
});

test('timed conversations, empty ones and ones without a record are placed as before', () => {
  state.conversations = [
    { name: 'timed', total_messages: 1, id: 'c0', untimed: 0 },
    { name: 'empty', total_messages: 0, id: 'c1', untimed: 0 },
    { name: 'no record', total_messages: 2, id: 'gone', untimed: 2 },
  ];
  state.messages = [{ conv: 0, ts: '2026-05-05T10:00:00Z' }];
  const blocks = buildBlocks();
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].start, '2026-05-05T10:00:00.000Z');
});
