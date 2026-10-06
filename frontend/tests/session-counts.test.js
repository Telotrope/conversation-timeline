// A session's stored counts under each view (core/session-counts.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §6).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { VIEWS, countedMessages, sessionRate, viewCounts, viewName } from '../core/session-counts.js';
import { counts } from './fixtures.js';

test('the two switches pick one of four views', () => {
  assert.deepEqual([viewName(true, true), viewName(true, false), viewName(false, true), viewName(false, false)], VIEWS);
});

test('each view reads its own counts; neither counts nothing', () => {
  const c = counts({ messages: 5, reviewed: 2, automatic: { caps: 1, any: 1 }, yours: { angry: 2, any: 2 }, both: { angry: 2, caps: 1, any: 3 } });
  assert.equal(viewCounts(c, 'automatic').caps, 1);
  assert.equal(viewCounts(c, 'yours').angry, 2);
  assert.equal(viewCounts(c, 'both').any, 3);
  assert.deepEqual(viewCounts(c, 'neither'), { caps: 0, critical: 0, angry: 0, any: 0 });
  assert.deepEqual(VIEWS.map((v) => countedMessages(c, v)), [5, 5, 2, 0]);
  assert.deepEqual(sessionRate(c, 'yours'), { total: 2, flagged: 2 });
  assert.deepEqual(sessionRate(c, 'neither'), { total: 0, flagged: 0 });
});
