import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import {
  pearsonR, computeFrictionAnalysis, computeTrendAnalysis, computeLengthAnalysis,
  computeTimeOfDayAnalysis, computeIdleGapAnalysis,
} from '../core/analyses.js';
import { attachFlags } from '../core/flags.js';
import { resetState, human, runChunked } from './fixtures.js';

// Two conversations. "Busy" has two sessions a day apart, one flagged
// message in the second; "Quiet" has one unflagged session.
function load() {
  const a1 = human(0, '2026-03-02T10:00:00Z');
  const a2 = human(0, '2026-03-03T09:00:00Z', { angry: true });
  const a3 = human(0, '2026-03-03T09:10:00Z');
  const b1 = human(1, '2026-03-09T20:00:00Z');
  state.conversations = [{ name: 'Busy', total_messages: 3 }, { name: 'Quiet', total_messages: 1 }, { name: 'Empty', total_messages: 0 }];
  state.humanMessages = [a1, a2, a3, b1];
  state.blocks = [
    { conv: 0, date: '2026-03-02', start: a1.ts, end: a1.ts, duration_sec: 0, count: 1 },
    { conv: 0, date: '2026-03-03', start: a2.ts, end: a3.ts, duration_sec: 600, count: 2 },
    { conv: 1, date: '2026-03-09', start: b1.ts, end: b1.ts, duration_sec: 0, count: 1 },
  ];
  attachFlags();
}

beforeEach(() => { resetState(); load(); });

test('pearsonR needs two points and some spread in both', () => {
  assert.equal(pearsonR([1], [1]), null);
  assert.equal(pearsonR([1, 1], [1, 2]), null);
  assert.equal(pearsonR([1, 2, 3], [2, 4, 6]), 1);
  assert.equal(pearsonR([1, 2, 3], [6, 4, 2]), -1);
});

test('friction by conversation ranks by share flagged and skips empty conversations', async () => {
  const { rows, granularity } = await computeFrictionAnalysis({}, runChunked);
  assert.equal(granularity, 'conversation');
  assert.deepEqual(rows.map((r) => [r.label, r.total, r.flagged, Math.round(r.pct)]), [
    ['Busy', 3, 1, 33], ['Quiet', 1, 0, 0],
  ]);
});

test('friction by session ranks sessions and carries their time span', async () => {
  const { rows } = await computeFrictionAnalysis({ granularity: 'session' }, runChunked);
  assert.equal(rows[0].conv, 0);
  assert.equal(rows[0].flagged, 1);
  assert.equal(rows[0].pct, 50);
  assert.equal(rows[0].rangeStart, Date.parse('2026-03-03T09:00:00Z'));
  assert.match(rows[0].label, /^Busy — Tuesday/);
  assert.deepEqual(rows.slice(1).map((r) => r.flagged), [0, 0]);
});

test('the trend buckets by week by default, or by month', async () => {
  const weekly = await computeTrendAnalysis({}, runChunked);
  assert.equal(weekly.granularity, 'week');
  assert.deepEqual(weekly.points.map((p) => [p.x, p.total, p.flagged]), [
    ['2026-W09', 3, 1], ['2026-W10', 1, 0],
  ]);
  const monthly = await computeTrendAnalysis({ granularity: 'month' }, runChunked);
  assert.deepEqual(monthly.points.map((p) => [p.x, p.total, p.flagged]), [['2026-03', 4, 1]]);
});

test('session length pairs minutes with the share flagged', async () => {
  const { points } = await computeLengthAnalysis({}, runChunked);
  assert.deepEqual(points.map((p) => [p.x, p.y]), [[0, 0], [10, 50], [0, 0]]);
});

test('time of day counts by hour and weekday', async () => {
  const { byHour, byDow } = await computeTimeOfDayAnalysis({}, runChunked);
  assert.deepEqual(byHour[9], { total: 2, flagged: 1 });
  assert.deepEqual(byHour[20], { total: 1, flagged: 0 });
  assert.deepEqual(byDow[2], { total: 2, flagged: 1 }); // Tuesday
});

test('idle gap measures from the previous session in the same conversation', async () => {
  const { points, excludedCount } = await computeIdleGapAnalysis({}, runChunked);
  assert.equal(points.length, 1);
  assert.equal(points[0].x, 23);
  assert.equal(points[0].y, 50);
  assert.equal(excludedCount, 2);
});

test('idle gap floors a zero gap so a log scale can draw it', async () => {
  state.blocks[1].start = state.blocks[0].end;
  state.blocks[1].count = 0;
  const { points } = await computeIdleGapAnalysis({}, runChunked);
  assert.equal(points[0].x, 0.01);
  assert.equal(points[0].y, 0);
});
