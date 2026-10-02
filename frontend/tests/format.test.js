import { test } from 'node:test';
import assert from 'node:assert/strict';
import { formatBytes, formatEta, formatDuration, formatClock, formatDayHeading, formatMonthHeading } from '../core/format.js';

test('formatBytes picks B, KB or MB', () => {
  assert.equal(formatBytes(512), '512 B');
  assert.equal(formatBytes(2048), '2 KB');
  assert.equal(formatBytes(3 * 1024 * 1024), '3.0 MB');
});

test('formatEta is deliberately coarse', () => {
  assert.equal(formatEta(Infinity), '');
  assert.equal(formatEta(-1), '');
  assert.equal(formatEta(3), 'almost done');
  assert.equal(formatEta(23), 'about 25 seconds left');
  assert.equal(formatEta(90), 'about a minute left');
  assert.equal(formatEta(600), 'about 10 minutes left');
});

test('formatDuration shows seconds, minutes, then hours', () => {
  assert.equal(formatDuration(42), '42s');
  assert.equal(formatDuration(125), '2m 5s');
  assert.equal(formatDuration(3 * 3600 + 7 * 60), '3h 7m');
});

test('the date and time formats name the day, month and year', () => {
  assert.match(formatClock('2026-03-02T15:04:00Z'), /3:04/);
  assert.match(formatDayHeading('2026-03-02'), /Monday.*March.*2.*2026/);
  assert.match(formatMonthHeading('2026-03-02'), /March.*2026/);
});
