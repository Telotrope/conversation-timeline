import { test } from 'node:test';
import assert from 'node:assert/strict';
import { localDateKey, localDaysTouched } from '../core/blocks.js';

test('localDateKey is the local calendar date', () => {
  assert.equal(localDateKey(new Date('2026-03-02T23:59:00Z')), '2026-03-02');
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
