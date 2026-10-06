// The budgets every waiting loop asks between steps (core/work-budget.js;
// plan docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { REPORT_MS, TURN_MS, clockBudget, stepBudget } from '../core/work-budget.js';

test('a clock budget allows steps until its time has passed', () => {
  let now = 1000;
  const budget = clockBudget(50, () => now);
  assert.equal(budget.allows(), true);
  now = 1049;
  assert.equal(budget.allows(), true);
  now = 1050;
  assert.equal(budget.allows(), false);
  assert.equal(clockBudget(0).allows(), false, 'the real clock: no time left at once');
  assert.deepEqual([TURN_MS, REPORT_MS], [50, 500]);
});

test('a step budget allows exactly its number of steps', () => {
  const budget = stepBudget(2);
  assert.deepEqual([budget.allows(), budget.allows(), budget.allows()], [true, true, false]);
  assert.equal(stepBudget(0).allows(), false);
});
