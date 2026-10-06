// The three analyses computed in the page stop part-way and carry on
// (core/analyses.js's createAnalysisRuns; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c): a run is
// kept under its analysis, options, view and data version, and asked for
// again it carries on from where it stopped, ending with the same numbers
// as a run that never stopped.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PAGE_ANALYSES, analysisSteps, createAnalysisRuns } from '../core/analyses.js';
import { runInTurns } from '../core/turns.js';
import { stepBudget } from '../core/work-budget.js';
import { block, counts, runSteps } from './fixtures.js';

const conversations = [{ name: 'A' }, { name: 'B' }];
const blocks = [
  block(0, 0, '2026-03-02T10:00:00Z', '2026-03-02T10:30:00Z', counts({ messages: 4, both: { caps: 1, any: 1 } })),
  block(0, 1, '2026-03-02T12:00:00Z', '2026-03-02T12:10:00Z', counts({ messages: 2, both: { angry: 2, any: 2 } })),
  block(1, 0, '2026-03-03T09:00:00Z', '2026-03-03T09:05:00Z', counts({ messages: 1 })),
];
const data = { conversations, blocks, view: 'both' };

test('the page computes friction, session length and idle time', () => {
  assert.deepEqual(PAGE_ANALYSES, ['friction', 'length', 'idlegap']);
});

for(const [name, opts] of [['friction', {}], ['friction', { granularity: 'session' }], ['length', {}], ['idlegap', {}]]){
  test(`${name} ${JSON.stringify(opts)}: stopped after each step and carried on, it ends as one run does`, async () => {
    const whole = runSteps(analysisSteps(name, opts, data));
    const runs = createAnalysisRuns();
    for(let turn = 0; ; turn++){
      const run = runs.get(name, opts, data, 7);
      if(run.finished){
        assert.deepEqual(run.result, whole);
        assert.ok(turn > 1, 'it took several visits');
        return;
      }
      let stop = false;
      const out = await runInTurns(run.steps, {
        budget: () => stepBudget(0), nextTurn: async () => { stop = true; }, stopped: () => stop,
      });
      if(out.finished){
        run.finished = true;
        run.result = out.result;
      }
    }
  });
}

test('a run is kept per analysis, options, view and data version, and forgotten for other sessions', () => {
  const runs = createAnalysisRuns();
  const first = runs.get('friction', {}, data, 1);
  assert.equal(runs.get('friction', {}, data, 1), first);
  assert.notEqual(runs.get('friction', { granularity: 'session' }, data, 1), first);
  assert.notEqual(runs.get('friction', {}, { ...data, view: 'yours' }, 1), first);
  assert.notEqual(runs.get('friction', {}, data, 2), first);
  const again = runs.get('friction', {}, { ...data, blocks: [...blocks] }, 1);
  assert.notEqual(again, first);
});

test('with neither switch on, nothing has a rate', () => {
  const none = { ...data, view: 'neither' };
  assert.deepEqual(runSteps(analysisSteps('friction', {}, none)).rows, []);
  assert.deepEqual(runSteps(analysisSteps('length', {}, none)).points, []);
  assert.deepEqual(runSteps(analysisSteps('idlegap', {}, none)), { points: [], excludedCount: 2, uncountedCount: 1 });
});
