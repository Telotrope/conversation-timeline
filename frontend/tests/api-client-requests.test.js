// The requests of plan docs/plans/2026-10-06-load-only-what-the-page-shows.md
// (infra/api-client.js): each answer in parts asked for with its cursor,
// the flag save with the handle from GET /messages, the file's text read
// from its short-lived address, and each failure turned into an error
// naming what failed. `fetch` answers as the server would.

import { test } from 'node:test';
import assert from 'node:assert/strict';

globalThis.window = { location: { search: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };

const api = await import('../infra/api-client.js');
const { connectActivitySink } = await import('../core/activity-sink.js');

const BASE = 'http://127.0.0.1:3000';

function answer(status, body){
  return {
    ok: status >= 200 && status < 300, status, headers: new Headers(),
    json: async () => body, text: async () => (typeof body === 'string' ? body : JSON.stringify(body)),
  };
}

// Answers each request with the next of `answers` (a status alone is an
// error answer), recording what was asked.
function serve(...answers){
  const asked = [];
  globalThis.fetch = async (url, init = {}) => {
    asked.push({ path: url.replace(BASE, ''), method: init.method || 'GET', body: init.body, auth: init.headers && init.headers.Authorization });
    const next = answers.shift();
    if(next instanceof Error) throw next;
    return typeof next === 'number' ? answer(next, { error: 'nope' }) : answer(200, next);
  };
  return asked;
}

test('a query string leaves out what is null or missing', () => {
  assert.equal(api.withQuery('/x', {}), '/x');
  assert.equal(api.withQuery('/x', { a: null, b: undefined, c: 0, d: 'é &' }), '/x?c=0&d=%C3%A9+%26');
});

test('records and files are read in every part, files sorted newest first', async () => {
  let asked = serve(
    { conversations: [{ conversation_id: 'a' }], total: 2, cursor: 'k', data_version: 1 },
    { conversations: [{ conversation_id: 'b' }], total: 2, cursor: null, data_version: 1 },
  );
  assert.deepEqual((await api.fetchConversationRecords('tok')).map((r) => r.conversation_id), ['a', 'b']);
  assert.deepEqual(asked.map((a) => a.path), ['/conversations', '/conversations?cursor=k']);
  assert.equal(asked[0].auth, 'Bearer tok');
  asked = serve(
    { uploads: [{ upload_id: 'old', uploaded_at: '2026-01-01T00:00:00Z' }], total: 2, cursor: 'u', data_version: 1 },
    { uploads: [{ upload_id: 'new', uploaded_at: '2026-01-01T00:00:00.5Z' }], total: 2, cursor: null, data_version: 1 },
  );
  assert.deepEqual((await api.fetchUploads('tok')).map((u) => u.upload_id), ['new', 'old']);
  assert.deepEqual(asked.map((a) => a.path), ['/uploads', '/uploads?cursor=u']);
});

test('each other part is asked for at its route, with its cursor or query', async () => {
  const asked = serve({}, {}, {}, {}, {}, {}, {});
  await api.fetchSessionsPart('tok', 's1');
  await api.fetchMessagesPart('tok', { conversation: 'c', rows: 50, until: 'end', cursor: null });
  await api.fetchConversationFilesPart('tok', 'c 1');
  await api.fetchFileAddress('tok', 'c', 'm', 2);
  await api.fetchAnalysis('tok', 'time-of-day', { view: 'both', tz: 'America/New_York' });
  await api.fetchExportPart('tok');
  await api.fetchConversationsPart('tok');
  assert.deepEqual(asked.map((a) => a.path), [
    '/sessions?cursor=s1', '/messages?conversation=c&rows=50&until=end', '/conversations/c%201/files',
    '/files/c/m/2', '/analyses/time-of-day?view=both&tz=America%2FNew_York', '/export', '/conversations',
  ]);
});

test('the scan sends its cursor back, and records which part it is', async () => {
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  const asked = serve({ sessions_done: 1, cursor: 'd1' }, { sessions_done: 2, cursor: null });
  assert.equal((await api.postDetect('tok', null, 0)).cursor, 'd1');
  await api.postDetect('tok', 'd1', 1);
  connectActivitySink(null);
  assert.deepEqual(asked.map((a) => [a.method, a.path, a.body]), [['POST', '/detect', '{}'], ['POST', '/detect', '{"cursor":"d1"}']]);
  assert.deepEqual(events.map((e) => e.part), [0, 1]);
  serve(500);
  await assert.rejects(api.postDetect('tok', null, 0), /scanning your messages failed \(500\): nope/);
});

test("a file's details are saved part by part until the server has rewritten every conversation", async () => {
  const parts = [];
  const asked = serve(
    { conversations: [{ conversation_id: 'a' }], done: 1, total: 2, cursor: 'e1', data_version: 1 },
    { conversations: [{ conversation_id: 'b' }], done: 2, total: 2, cursor: null, data_version: 2 },
  );
  const records = await api.saveFileMetadata('tok', 'u1', { medium: { kind: 'typed' } }, (p) => parts.push(p.done));
  assert.deepEqual(records.map((r) => r.conversation_id), ['a', 'b']);
  assert.deepEqual(parts, [1, 2]);
  assert.deepEqual(asked.map((a) => [a.method, a.path, JSON.parse(a.body)]), [
    ['PUT', '/uploads/u1/metadata', { medium: { kind: 'typed' } }],
    ['PUT', '/uploads/u1/metadata', { medium: { kind: 'typed' }, cursor: 'e1' }],
  ]);
  serve({ conversation_id: 'c' });
  assert.deepEqual(await api.saveConversationMetadata('tok', 'c', {}), { conversation_id: 'c' });
  serve({ conversations: [], done: 0, total: 0, cursor: null, data_version: 1 });
  assert.deepEqual(await api.saveFileMetadata('tok', 'u2', {}), []);
});

test("a stored file's text is read from its address, recorded without it", async () => {
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  let asked = serve('print(1)');
  assert.equal(await api.downloadFileText('/_dev/local-storage/get/files/x'), 'print(1)');
  assert.equal(asked[0].path, '/_dev/local-storage/get/files/x');
  asked = serve(404);
  await assert.rejects(api.downloadFileText('https://s3.example/files/x?sig=1'), (e) => e.status === 404 && /downloading the file failed \(404\)/.test(e.message));
  assert.equal(asked[0].path, 'https://s3.example/files/x?sig=1');
  serve(new TypeError('Failed to fetch'));
  await assert.rejects(api.downloadFileText('https://s3.example/files/x'), /Failed to fetch/);
  connectActivitySink(null);
  assert.deepEqual(events.map((e) => [e.route, e.status, e.error_kind]), [
    ['s3 GET files/…', 200, undefined], ['s3 GET files/…', 404, undefined], ['s3 GET files/…', null, 'network'],
  ]);
  assert.equal(JSON.stringify(events).includes('sig=1'), false);
});

test('a flag save sends the handle and answers with the counted session; refusals and failures are outcomes', async () => {
  serve({ token: 'tok' });
  await api.ensureAuthToken('alice');
  const msg = { conversationId: 'c', messageId: 'm', handle: 'h1' };
  const reply = { message_id: 'm', session: { number: 0 }, data_version: 9 };
  const asked = serve(reply);
  assert.deepEqual(await api.patchFlagsToBackend(msg, { caps: true }), { outcome: api.SaveOutcome.SAVED, reply });
  assert.deepEqual([asked[0].method, asked[0].path, JSON.parse(asked[0].body)], ['PATCH', '/conversations/c/messages/m/flags', { caps: true, handle: 'h1' }]);
  serve(403);
  assert.deepEqual(await api.patchFlagsToBackend(msg, {}), { outcome: api.SaveOutcome.STALE_PAGE, status: 403 });
  serve(500);
  assert.deepEqual(await api.patchFlagsToBackend(msg, {}),
    { outcome: api.SaveOutcome.SERVER_ERROR, detail: 'server returned 500', status: 500, errorKind: 'server_error' });
  serve(new TypeError('Failed to fetch'));
  assert.deepEqual(await api.patchFlagsToBackend(msg, {}),
    { outcome: api.SaveOutcome.SERVER_ERROR, detail: 'Failed to fetch', status: null, errorKind: 'network' });
});
