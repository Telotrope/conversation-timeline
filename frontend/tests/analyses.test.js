import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import { pearsonR, analysisSteps } from '../core/analyses.js';
import { resetState, block, counts, runSteps } from './fixtures.js';

// Two conversations. "Busy" has two sessions a day apart, one flagged
// message in the second; "Quiet" has one unflagged session. Each session
// carries its stored counts, as the server gives them.
function load() {
  state.conversations = [{ name: 'Busy', total_messages: 3 }, { name: 'Quiet', total_messages: 1 }, { name: 'Empty', total_messages: 0 }];
  state.blocks = [
    block(0, 0, '2026-03-02T10:00:00Z', '2026-03-02T10:00:00Z', counts({ messages: 1 })),
    block(0, 1, '2026-03-03T09:00:00Z', '2026-03-03T09:10:00Z',
      counts({ messages: 2, automatic: { angry: 1, any: 1 }, both: { angry: 1, any: 1 } })),
    block(1, 0, '2026-03-09T20:00:00Z', '2026-03-09T20:00:00Z', counts({ messages: 1 })),
  ];
}

// The analysis's result under the view the show switches pick.
function run(name, opts = {}, view = 'both') {
  return runSteps(analysisSteps(name, opts, { conversations: state.conversations, blocks: state.blocks, view }));
}

beforeEach(() => { resetState(); load(); });

test('pearsonR needs two points and some spread in both', () => {
  assert.equal(pearsonR([1], [1]), null);
  assert.equal(pearsonR([1, 1], [1, 2]), null);
  assert.equal(pearsonR([1, 2, 3], [2, 4, 6]), 1);
  assert.equal(pearsonR([1, 2, 3], [6, 4, 2]), -1);
});

test('friction by conversation ranks by share flagged and skips empty conversations', () => {
  const { rows, granularity } = run('friction');
  assert.equal(granularity, 'conversation');
  assert.deepEqual(rows.map((r) => [r.label, r.total, r.flagged, Math.round(r.pct)]), [
    ['Busy', 3, 1, 33], ['Quiet', 1, 0, 0],
  ]);
});

test('friction by session ranks sessions and carries their time span', () => {
  const { rows } = run('friction', { granularity: 'session' });
  assert.equal(rows[0].conv, 0);
  assert.equal(rows[0].flagged, 1);
  assert.equal(rows[0].pct, 50);
  assert.equal(rows[0].rangeStart, Date.parse('2026-03-03T09:00:00Z'));
  assert.match(rows[0].label, /^Busy — Tuesday/);
  assert.deepEqual(rows.slice(1).map((r) => r.flagged), [0, 0]);
});

test('session length pairs minutes with the share flagged', () => {
  const { points } = run('length');
  assert.deepEqual(points.map((p) => [p.x, p.y]), [[0, 0], [10, 50], [0, 0]]);
});

test('idle gap measures from the previous session in the same conversation', () => {
  const { points, excludedCount } = run('idlegap');
  assert.equal(points.length, 1);
  assert.equal(points[0].x, 23);
  assert.equal(points[0].y, 50);
  assert.equal(excludedCount, 2);
});

test('the idle time before a session is the pause since the previous session ended', () => {
  // Busy's second session starts 20 minutes after its first one ended.
  state.blocks[1].start = '2026-03-02T10:20:00Z';
  const { points } = run('idlegap');
  assert.equal(points[0].x, 20 / 60);
  assert.equal(points[0].y, 50);
});

// With automatic tags hidden, only reviewed messages count. Here only the
// flagged message in Busy's second session and one other are reviewed.
function reviewOnly() {
  state.blocks[1].counts = counts({
    messages: 2, reviewed: 2, automatic: { angry: 1, any: 1 }, yours: { angry: 1, any: 1 }, both: { angry: 1, any: 1 },
  });
}

test('with only your tags, unreviewed conversations and sessions are left out', () => {
  reviewOnly();
  const byConv = run('friction', {}, 'yours');
  assert.deepEqual(byConv.rows.map((r) => [r.label, r.total, r.flagged, r.pct]), [['Busy', 2, 1, 50]]);
  const bySession = run('friction', { granularity: 'session' }, 'yours');
  assert.deepEqual(bySession.rows.map((r) => [r.total, r.flagged]), [[2, 1]]);
});

test('with only your tags, session length counts reviewed messages only', () => {
  reviewOnly();
  const length = run('length', {}, 'yours');
  assert.deepEqual(length.points.map((p) => [p.x, p.y]), [[10, 50]]);
});

test('idle gap skips later sessions with nothing reviewed, and says how many', () => {
  let r = run('idlegap', {}, 'yours');
  assert.deepEqual([r.points.length, r.excludedCount, r.uncountedCount], [0, 2, 1]);
  reviewOnly();
  r = run('idlegap', {}, 'yours');
  assert.deepEqual([r.points.length, r.uncountedCount], [1, 0]);
});

test('session rates count only your messages, not Claude\'s', () => {
  // A session's count includes Claude's replies; rates must not.
  state.blocks[1].count = 10;
  const { rows } = run('friction', { granularity: 'session' });
  assert.equal(rows[0].pct, 50);
});
