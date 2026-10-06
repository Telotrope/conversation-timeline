// infra/api-client.js's failure and edge paths that no browser test reaches
// (plan docs/plans/completed/2026-10-05-page-coverage-gaps.md). The module reads the
// address's query string and localStorage as it loads, so stand-ins for
// those two are set first; `fetch` is replaced per test. Node runs each test
// file in its own process, so none of this reaches other test files.

import { test } from 'node:test';
import assert from 'node:assert/strict';

const stored = new Map();
globalThis.window = { location: { search: '' } };
globalThis.localStorage = {
  getItem: (k) => (stored.has(k) ? stored.get(k) : null),
  setItem: (k, v) => stored.set(k, String(v)),
};

const api = await import('../infra/api-client.js');
const { connectActivitySink } = await import('../core/activity-sink.js');

// What the page recorded while `fn` ran.
async function recorded(fn){
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  try{
    return { result: await fn(), events };
  } finally {
    connectActivitySink(null);
  }
}

function answer(status, { body = '', headers = {} } = {}){
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: new Headers(headers),
    text: async () => body,
    json: async () => JSON.parse(body),
  };
}

test('an error reply whose body cannot be read still names the status and why', async () => {
  const res = { status: 502, text: async () => { throw new TypeError('body already used'); } };
  assert.equal(
    await api.describeFailure('checking on the upload', res),
    "checking on the upload failed (502), and the error response itself couldn't be read: body already used",
  );
});

test('a flag save while signed out is not sent', async () => {
  let sent = false;
  globalThis.fetch = async () => { sent = true; return answer(200); };
  assert.deepEqual(await api.patchFlagsToBackend({ conv: 0, rawIndex: 0 }, { caps: true }),
    { outcome: api.SaveOutcome.NOT_LOGGED_IN });
  assert.equal(sent, false);
});

test('a flag save for a message the server issued no handle for is not sent', async () => {
  globalThis.fetch = async () => answer(200, { body: JSON.stringify({ token: 'tok' }) });
  await api.ensureAuthToken('alice');
  let sent = false;
  globalThis.fetch = async () => { sent = true; return answer(200); };
  // A row of GET /messages always carries its handle; one without can't be saved.
  assert.deepEqual(await api.patchFlagsToBackend({ conversationId: 'c', messageId: 'm', handle: '' }, { caps: true }),
    { outcome: api.SaveOutcome.NO_SERVER_ID });
  assert.equal(sent, false);
});

test('a part of the annotated download answered with an error is thrown with its status and recorded', async () => {
  globalThis.fetch = async () => answer(500, { body: 'oops' });
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  await assert.rejects(api.fetchExportPart('tok', 'c1'), (err) => err.status === 500 && err.kind === 'server_error'
    && err.message === 'building your annotated download failed (500): oops');
  connectActivitySink(null);
  assert.equal(events.length, 1);
  assert.equal(events[0].route, '/export');
  assert.equal(events[0].status, 500);
});

test('a part of the annotated download that gets no answer is recorded by its kind and thrown on', async () => {
  globalThis.fetch = async () => { throw new TypeError('Failed to fetch'); };
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  await assert.rejects(api.fetchExportPart('tok'), /Failed to fetch/);
  connectActivitySink(null);
  assert.equal(events.length, 1);
  assert.equal(events[0].status, null);
  assert.equal(events[0].error_kind, 'network');
  assert.equal(JSON.stringify(events[0]).includes('Failed to fetch'), false, 'the error text is not recorded');
});

test("an activity batch is sent with the session and sign-in, and its answer returned", async () => {
  let request;
  globalThis.fetch = async (url, init) => { request = { url, init }; return answer(204); };
  const reply = await api.postActivityBatch('{"events":[]}', { token: 'tok', keepalive: false });
  assert.deepEqual(reply, { ok: true, status: 204 });
  assert.equal(request.url, 'http://127.0.0.1:3000/activity');
  assert.equal(request.init.headers.Authorization, 'Bearer tok');
  assert.equal(request.init.headers['x-timeline-session'], api.SESSION_ID);
});
