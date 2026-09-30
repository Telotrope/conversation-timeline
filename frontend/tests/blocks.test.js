import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import { buildBlocks, localDateKey, localDaysTouched } from '../core/blocks.js';
import { resetState } from './fixtures.js';

beforeEach(resetState);

test('localDateKey is the local calendar date', () => {
  assert.equal(localDateKey(new Date('2026-03-02T23:59:00Z')), '2026-03-02');
});

test('a gap of 15 minutes or more starts a new session; a shorter one does not', () => {
  state.messages = [
    { conv: 0, ts: '2026-01-01T10:00:00Z' },
    { conv: 0, ts: '2026-01-01T10:14:59Z' },
    { conv: 0, ts: '2026-01-01T10:29:59Z' },
    { conv: 0, ts: '2026-01-01T10:30:00Z' },
  ];
  const blocks = buildBlocks();
  assert.deepEqual(blocks, [
    { conv: 0, date: '2026-01-01', start: '2026-01-01T10:00:00.000Z', end: '2026-01-01T10:14:59.000Z', duration_sec: 899, count: 2 },
    { conv: 0, date: '2026-01-01', start: '2026-01-01T10:29:59.000Z', end: '2026-01-01T10:30:00.000Z', duration_sec: 1, count: 2 },
  ]);
});

test('a session continues across midnight, and never spans two conversations', () => {
  state.messages = [
    { conv: 1, ts: '2026-01-01T23:55:00Z' },
    { conv: 1, ts: '2026-01-02T00:05:00Z' },
    { conv: 2, ts: '2026-01-01T23:59:30Z' },
  ];
  const blocks = buildBlocks();
  assert.deepEqual(blocks.map((b) => [b.conv, b.date, b.count, b.duration_sec]), [
    [1, '2026-01-01', 2, 600], [2, '2026-01-01', 1, 0],
  ]);
});

test('localDaysTouched gives each day\'s part of a session', () => {
  const pieces = localDaysTouched({ start: '2026-01-01T23:55:00Z', end: '2026-01-02T00:05:00Z' });
  assert.deepEqual(pieces.map((p) => [p.date, p.start.toISOString(), p.end.toISOString()]), [
    ['2026-01-01', '2026-01-01T23:55:00.000Z', '2026-01-02T00:00:00.000Z'],
    ['2026-01-02', '2026-01-02T00:00:00.000Z', '2026-01-02T00:05:00.000Z'],
  ]);
  const one = localDaysTouched({ start: '2026-01-01T10:00:00Z', end: '2026-01-01T10:30:00Z' });
  assert.deepEqual(one.map((p) => p.date), ['2026-01-01']);
});
