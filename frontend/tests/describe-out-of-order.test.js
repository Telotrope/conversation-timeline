// Describe's warning about messages timed earlier than the message before
// them in the file (ui/describe-form.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §12.3): bad data
// you are told about, for a file's conversations or one conversation. The
// page is a stand-in; `fetch` is a scripted server.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';

const record = (id, outOfOrder, name = id) => ({
  conversation_id: id, name, untimed: 0, out_of_order: outOfOrder, source: { upload_id: 'u1', file_name: 'f.json' },
  span: { start: '2026-03-01T10:00:00Z', end: '2026-03-01T11:00:00Z' },
  participants: [{ kind: 'human', name: 'Al' }, { kind: 'claude' }], medium: { kind: 'typed' }, details_origin: 'guessed',
});
const upload = { upload_id: 'u1', file_name: 'f.json', uploaded_at: '2026-03-02T00:00:00Z', conversation_count: 3,
  participants: [{ kind: 'human', name: 'Al' }, { kind: 'claude' }], medium: { kind: 'typed' } };
globalThis.fetch = async (url) => {
  const { pathname } = new URL(url);
  const ok = (body) => ({ ok: true, status: 200, headers: new Headers(), json: async () => body });
  if(pathname === '/_dev/login') return ok({ token: 'tok' });
  if(pathname === '/uploads') return ok({ uploads: [upload], total: 1, cursor: null, data_version: 1 });
  if(pathname === '/conversations') {
    return ok({ conversations: [record('c1', 2), record('c2', 1, ''), record('c3', 0)], total: 3, cursor: null, data_version: 1 });
  }
  throw new Error(`nothing scripted for ${pathname}`);
};

const describe = await import('../ui/describe-form.js');

function warnings(node = page.el('describeBody'), found = []){
  if(node.classes && node.classes.has('untimed-warning')) found.push(node.textContent);
  for(const child of node.children || []) if(typeof child === 'object') warnings(child, found);
  return found;
}

test("a file's conversations with times out of order are named, with how many messages", async () => {
  describe.connectDescribe({ done(){}, cancel(){}, ranOut(){} });
  await describe.openDescribe({ kind: 'file', uploadId: 'u1' });
  assert.deepEqual(warnings(), [
    '3 messages in c1, (untitled) have times earlier than the message before them in the file; their order is kept as the file gives it.',
  ]);
});

test("one conversation's message out of order is counted in its warning; none, no warning", async () => {
  await describe.openDescribe({ kind: 'conversation', conversationId: 'c2' });
  assert.deepEqual(warnings(), [
    '1 message in this conversation has a time earlier than the message before it in the file; their order is kept as the file gives it.',
  ]);
  await describe.openDescribe({ kind: 'conversation', conversationId: 'c3' });
  assert.deepEqual(warnings(), []);
});
