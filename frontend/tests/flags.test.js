import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { state } from '../core/state.js';
import { hasUserValue, effectiveFlag, isOverridden, isFlagged, isReviewed } from '../core/flags.js';
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

test('a row is reviewed once any of its flags has your value', () => {
  const m = human(0, '2026-01-01T10:00:00Z');
  assert.equal(isReviewed(m), false);
  state.overrides[m.id] = { caps: false };
  assert.equal(isReviewed(m), true);
});
