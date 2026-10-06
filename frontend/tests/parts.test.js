// Reading an answer given in parts (core/parts.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { MAX_RESTARTS, joinParts, readAllParts } from '../core/parts.js';

// A server answering from `script`: one list of parts per reading.
function server(parts){
  const asked = [];
  return {
    asked,
    fetchPart: async (cursor) => { asked.push(cursor); return parts.shift(); },
  };
}

test('every part is asked for with the cursor before it, until a part has none', async () => {
  const s = server([
    { items: [1], cursor: 'a', data_version: 3 },
    { items: [2, 3], cursor: 'b', data_version: 3 },
    { items: [], data_version: 3 },
  ]);
  const seen = [];
  const { parts, dataVersion } = await readAllParts(s.fetchPart, { onPart: (part, so) => seen.push(so.length) });
  assert.deepEqual(s.asked, [null, 'a', 'b']);
  assert.deepEqual(joinParts(parts, 'items'), [1, 2, 3]);
  assert.equal(dataVersion, 3);
  assert.deepEqual(seen, [1, 2, 3]);
});

test('a changed data version starts the reading again from the beginning', async () => {
  const s = server([
    { items: ['old'], cursor: 'a', data_version: 1 },
    { items: ['x'], cursor: null, data_version: 2 },
    { items: ['new'], cursor: null, data_version: 2 },
  ]);
  let restarts = 0;
  const { parts } = await readAllParts(s.fetchPart, { onRestart: () => { restarts += 1; } });
  assert.deepEqual(s.asked, [null, 'a', null]);
  assert.deepEqual(joinParts(parts, 'items'), ['new']);
  assert.equal(restarts, 1);
});

test('data that keeps changing gives up after a few readings, saying why', async () => {
  let version = 0;
  const fetchPart = async (cursor) => ({ items: [], cursor: cursor === null ? 'more' : null, data_version: version++ });
  await assert.rejects(readAllParts(fetchPart), /your data kept changing while it was being read/);
  assert.equal(version, (MAX_RESTARTS + 1) * 2);
});
