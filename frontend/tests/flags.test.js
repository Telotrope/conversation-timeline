import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import { hasUserValue, effectiveFlag, isOverridden, isFlagged, attachFlags, isReviewed, countsTowardRates } from '../core/flags.js';
import { resetState, human } from './fixtures.js';

beforeEach(resetState);

test('with both switches on, your correction wins and automatic fills the gaps', () => {
  const m = human(0, '2026-01-01T10:00:00Z', { caps: true, angry: true });
  state.overrides[m.id] = { caps: false };
  assert.equal(hasUserValue(m, 'caps'), true);
  assert.equal(hasUserValue(m, 'angry'), false);
  assert.equal(isOverridden(m, 'caps'), true);
  assert.equal(effectiveFlag(m, 'caps'), false);
  assert.equal(effectiveFlag(m, 'angry'), true);
});

test('automatic only ignores your corrections', () => {
  const m = human(0, '2026-01-01T10:00:00Z', { caps: true });
  state.overrides[m.id] = { caps: false };
  state.showUser = false;
  assert.equal(effectiveFlag(m, 'caps'), true);
});

test('yours only ignores automatic flags', () => {
  const m = human(0, '2026-01-01T10:00:00Z', { caps: true, angry: true });
  state.overrides[m.id] = { angry: true };
  state.showAuto = false;
  assert.equal(effectiveFlag(m, 'caps'), false);
  assert.equal(effectiveFlag(m, 'angry'), true);
});

test('with both switches off nothing is flagged', () => {
  const m = human(0, '2026-01-01T10:00:00Z', { critical: true });
  state.showAuto = false;
  state.showUser = false;
  assert.equal(effectiveFlag(m, 'critical'), false);
  assert.equal(isFlagged(m), false);
});

test('isFlagged is true for any one of the three flags', () => {
  for (const type of ['critical', 'angry', 'caps']) {
    assert.equal(isFlagged(human(0, '2026-01-01T10:00:00Z', { [type]: true })), true, type);
  }
  assert.equal(isFlagged(human(0, '2026-01-01T10:00:00Z')), false);
});

test('attachFlags files each message under its session, or the nearest one', () => {
  state.blocks = [
    { conv: 0, start: '2026-01-01T10:00:00Z', end: '2026-01-01T10:10:00Z' },
    { conv: 0, start: '2026-01-01T12:00:00Z', end: '2026-01-01T12:10:00Z' },
    { conv: 1, start: '2026-01-01T10:00:00Z', end: '2026-01-01T10:00:00Z' },
  ];
  const inside = human(0, '2026-01-01T10:05:00Z', { critical: true, angry: true, caps: true });
  const early = human(0, '2026-01-01T09:00:00Z');
  const nearerSecond = human(0, '2026-01-01T11:50:00Z');
  const after = human(0, '2026-01-01T13:00:00Z');
  const noSessions = human(2, '2026-01-01T10:00:00Z', { critical: true });
  state.humanMessages = [after, inside, early, nearerSecond, noSessions];

  attachFlags();
  const [first, second, other] = state.blocks;
  assert.deepEqual(first.allHuman, [early, inside]);
  assert.deepEqual(first.criticalItems, [inside]);
  assert.deepEqual(first.angryItems, [inside]);
  assert.deepEqual(first.capsItems, [inside]);
  assert.deepEqual(second.allHuman, [nearerSecond, after]);
  assert.deepEqual(other.allHuman, []);
});

test('a row is reviewed once any of its flags has your value', () => {
  const m = human(0, '2026-01-01T10:00:00Z');
  assert.equal(isReviewed(m), false);
  state.overrides[m.id] = { caps: false };
  assert.equal(isReviewed(m), true);
});

test('only messages with a value under the switches count toward rates', () => {
  const reviewed = human(0, '2026-01-01T10:00:00Z');
  const unreviewed = human(0, '2026-01-01T10:01:00Z');
  state.overrides[reviewed.id] = { caps: false, angry: false, critical: false };
  assert.equal(countsTowardRates(unreviewed), true);
  state.showAuto = false;
  assert.equal(countsTowardRates(reviewed), true);
  assert.equal(countsTowardRates(unreviewed), false);
  state.showUser = false;
  assert.equal(countsTowardRates(reviewed), false);
});
