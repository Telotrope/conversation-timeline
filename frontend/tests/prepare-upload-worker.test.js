// The worker's own file (workers/prepare-upload.js): it takes the page's
// { file } and posts the pipeline's messages back. Node has no worker
// scope, so `self` is a stand-in with the two things the file uses. Its own
// test file: the worker's listener is set as the module loads.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const posted = [];
let listener = null;
globalThis.self = {
  addEventListener: (type, fn) => { assert.equal(type, 'message'); listener = fn; },
  postMessage: (m) => posted.push(m),
};
await import('../workers/prepare-upload.js');

test("the worker prepares the file the page posts, and posts the result back", async () => {
  const text = '[{"uuid":"a","chat_messages":[]}]';
  listener({ data: { file: new Blob([text]) } });
  for(let i = 0; i < 100 && !posted.some((m) => m.kind === 'done'); i++) await new Promise((r) => setTimeout(r, 5));
  const done = posted.find((m) => m.kind === 'done');
  assert.ok(done, JSON.stringify(posted));
  assert.equal(done.conversations, 1);
});
