// Page work in turns (core/turns.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b, §8c): each
// turn does steps until its budget says stop, reports, and hands the screen
// back; stopped between turns, the work carries on later from where it was.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runInTurns } from '../core/turns.js';
import { stepBudget } from '../core/work-budget.js';

function* count(n){
  for(let i = 1; i <= n; i++) yield { done: i, total: n };
  return 'finished';
}

test('every turn does its budget of steps and reports the last progress', async () => {
  const reports = [];
  let turns = 0;
  const out = await runInTurns(count(5), {
    budget: () => stepBudget(1), // two steps a turn: one, then one more allowed
    nextTurn: async () => { turns += 1; },
    onProgress: (p) => reports.push(p.done),
  });
  assert.deepEqual(out, { finished: true, result: 'finished' });
  assert.deepEqual(reports, [2, 4]);
  assert.equal(turns, 2);
});

test('a turn whose budget allows nothing still does one step', async () => {
  const reports = [];
  await runInTurns(count(2), { budget: () => stepBudget(0), nextTurn: async () => {}, onProgress: (p) => reports.push(p.done) });
  assert.deepEqual(reports, [1, 2]);
});

test('stopped between turns, the same steps carry on later and end the same', async () => {
  const steps = count(4);
  let stop = false;
  const first = await runInTurns(steps, {
    budget: () => stepBudget(0), nextTurn: async () => { stop = true; }, stopped: () => stop,
  });
  assert.deepEqual(first, { finished: false });
  const second = await runInTurns(steps, { budget: () => stepBudget(10), nextTurn: async () => {} });
  assert.deepEqual(second, { finished: true, result: 'finished' });
});
