// What the page accepts as an export, now that it reads a file only to slim
// it before sending (core/slim-export.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b, §10b). The
// rest of what this file tested moved to the server with the reading of
// exports.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSlimWriter, NotAnExport, rootKind } from '../core/slim-export.js';

// The slimmed text of a file whose top-level items are `items`, as the
// worker's parser hands them over.
function slimmed(kind, items){
  const writer = createSlimWriter(kind);
  return items.map((item) => writer.add(item, 0, kind === 'array' ? 1 : 2)).join('') + writer.finish();
}

test('a bare array is read as a raw export, and missing names and messages are tolerated', () => {
  const text = '[{ }, { "name": "B", "chat_messages": [] }]';
  assert.equal(rootKind(text), 'array');
  // Still a bare array, so the server treats it as a raw export; nothing is
  // made up for what the file lacks.
  assert.deepEqual(JSON.parse(slimmed('array', [{}, { name: 'B', chat_messages: [] }])), [{}, { name: 'B', chat_messages: [] }]);
});

test('anything other than an array or {conversations: [...]} is rejected', () => {
  assert.equal(rootKind('  "nope"'), 'other');
  assert.equal(rootKind('null'), 'other');
  assert.throws(() => createSlimWriter('other'), NotAnExport);
  // An object without conversations: nothing reached, so nothing to send slimmed.
  assert.throws(() => createSlimWriter('object').finish(), /holds no conversations/);
  assert.throws(() => createSlimWriter('array').add(5, 0, 1), /item 1 of the file isn't a conversation/);
});
