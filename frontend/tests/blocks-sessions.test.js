// Sessions as the views draw them, and a Calendar day as the instants the
// server is asked for (core/blocks.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b). TZ=UTC in
// these tests, so local days are UTC days.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { localDayBounds, toBlock } from '../core/blocks.js';
import { counts, session } from './fixtures.js';

test('a local day is asked for from its midnight to the last moment before the next', () => {
  assert.deepEqual(localDayBounds('2026-03-02'), { from: '2026-03-02T00:00:00.000Z', to: '2026-03-02T23:59:59.999Z' });
  assert.deepEqual(localDayBounds('2026-12-31'), { from: '2026-12-31T00:00:00.000Z', to: '2026-12-31T23:59:59.999Z' });
});

test("a session from the server becomes a block on its starting day, with its length and counts", () => {
  const c = counts({ messages: 3 });
  const b = toBlock(session('c1', 2, '2026-03-02T23:50:00Z', '2026-03-03T00:05:30Z', c, { message_count: 7, placement: 'span' }), 4);
  assert.deepEqual(b, {
    conv: 4, number: 2, date: '2026-03-02', start: '2026-03-02T23:50:00Z', end: '2026-03-03T00:05:30Z',
    duration_sec: 930, count: 7, placement: 'span', counts: c,
  });
});
