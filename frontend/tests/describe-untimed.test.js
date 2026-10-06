// Describe's warning about messages of unknown time, and the saving of a
// file's details in parts (ui/describe-form.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4e, §8c). The
// page is a stand-in; `fetch` is a scripted server.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';

const record = (id, upload, untimed, name = id) => ({
  conversation_id: id, name, untimed, source: { upload_id: upload, file_name: 'f.json' },
  span: { start: '2026-03-01T10:00:00Z', end: '2026-03-01T11:00:00Z' },
  participants: [{ kind: 'human', name: 'Al' }, { kind: 'claude' }], medium: { kind: 'typed' }, details_origin: 'guessed',
});
const upload = { upload_id: 'u1', file_name: 'f.json', uploaded_at: '2026-03-02T00:00:00Z', conversation_count: 3,
  participants: [{ kind: 'human', name: 'Al' }, { kind: 'claude' }], medium: { kind: 'typed' } };
const puts = [];
globalThis.fetch = async (url, init = {}) => {
  const { pathname } = new URL(url);
  const ok = (body) => ({ ok: true, status: 200, headers: new Headers(), json: async () => body });
  if(pathname === '/_dev/login') return ok({ token: 'tok' });
  if(pathname === '/uploads') return ok({ uploads: [upload], total: 1, cursor: null, data_version: 1 });
  if(pathname === '/conversations') {
    return ok({ conversations: [record('c1', 'u1', 2), record('c2', 'u1', 1, ''), record('c3', 'u1', 0), record('c4', 'u2', 1)], total: 4, cursor: null, data_version: 1 });
  }
  if(init.method === 'PUT'){
    const body = JSON.parse(init.body);
    puts.push(body);
    return ok({ conversations: [], done: body.cursor ? 3 : 1, total: 3, cursor: body.cursor ? null : 'e1', data_version: 2 });
  }
  throw new Error(`nothing scripted for ${pathname}`);
};

const describe = await import('../ui/describe-form.js');

// Every paragraph drawn with the warning's class, anywhere in the form.
function warnings(node = page.el('describeBody'), found = []){
  if(node.classes && node.classes.has('untimed-warning')) found.push(node.textContent);
  for(const child of node.children || []) if(typeof child === 'object') warnings(child, found);
  return found;
}

test("a file's conversations with messages of unknown time are named, with how many such messages", async () => {
  const done = [];
  describe.connectDescribe({ done: (s) => done.push(s), cancel(){}, ranOut(){} });
  await describe.openDescribe({ kind: 'file', uploadId: 'u1' });
  assert.deepEqual(warnings(), [
    'In 2 conversations (c1, (untitled)), 3 messages have no time. They are counted, but not placed by their own time: '
      + 'each such conversation is placed on the timeline by its start and end, which you can change from the Conversations tab.',
  ]);
  const statuses = [];
  const line = page.el('describeStatus');
  Object.defineProperty(line, 'textContent', { get(){ return this.text; }, set(v){ this.text = v; statuses.push(v); }, configurable: true });
  await describe.describeDone();
  delete line.textContent;
  assert.deepEqual(puts.map((p) => p.cursor), [undefined, 'e1'], 'sent again with the cursor until done');
  assert.ok(statuses.includes('Saving — 1 of 3 conversations updated…'), statuses.join(' | '));
  assert.ok(statuses.includes('Saving — 3 of 3 conversations updated…'));
  assert.equal(done.length, 1);
});

test("one conversation's messages of unknown time are counted in its warning; none, no warning", async () => {
  await describe.openDescribe({ kind: 'conversation', conversationId: 'c4' });
  assert.deepEqual(warnings(), [
    '1 message has no time. It is counted, but not placed by its own time: this conversation is placed on the timeline by the start and end below.',
  ]);
  await describe.openDescribe({ kind: 'conversation', conversationId: 'c3' });
  assert.deepEqual(warnings(), []);
});
