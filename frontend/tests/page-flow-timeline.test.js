// The page flow's arcs that read the timeline (ui/page-flow.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b, §8c): signing
// in asks only whether there are conversations (the first part's total);
// the timeline is read behind the loading modal when it opens, after an
// upload once its files are described, and again after a conversation's or
// a file's details are saved; a failure offers Try again. The page and the
// modules main.js hands in are stand-ins; `fetch` is a scripted server.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage, StubElement } from './page-stub.js';
import { state } from '../core/state.js';
import { PageError } from '../core/page-error.js';
import { resetState } from './fixtures.js';

const page = installPage();
const history = [];
globalThis.window = {
  location: { search: '', hash: '' },
  history: {
    pushState: (s, t, hash) => { history.push(hash); window.location.hash = hash; },
    replaceState: (s, t, hash) => { window.location.hash = hash; },
  },
};
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';
// The status line and bar's group moves into the modal and back.
const home = new StubElement('div');
home.insertBefore = (node) => home.children.push(node);
page.el('loadProgressGroup').parentNode = home;

let total = 2;
globalThis.fetch = async (url) => {
  const { pathname } = new URL(url);
  const ok = (body) => ({ ok: true, status: 200, headers: new Headers(), json: async () => body });
  if(pathname === '/_dev/login') return ok({ token: 'tok' });
  if(pathname === '/conversations') return ok({ conversations: [], total, cursor: null, data_version: 1 });
  if(pathname.endsWith('/files')) return ok({ files: [], sessions_done: 0, sessions_total: 0, cursor: null, data_version: 1 });
  throw new Error(`nothing scripted for ${pathname}`);
};

const flow = await import('../ui/page-flow.js');

const calls = [];
let loaded = true;
flow.connectPageFlow({
  signedIn: async () => true, accountLabel: async () => 'alice', signOut: async () => {}, refreshSignIn: () => {},
  resetUploadPage: (hasData) => calls.push(['resetUpload', hasData]), uploading: () => false,
  openDescribe: (subject) => calls.push(['describe', subject.kind]),
  loadTimeline: async (token) => {
    calls.push(['load', token]);
    if(loaded instanceof Error) throw loaded;
    return loaded;
  },
  applyLocationHash: () => calls.push(['hash', window.location.hash]),
  describeLoadFailure: (err) => ['load.failed', { detail: err.message, hint: '' }],
});

beforeEach(() => {
  resetState();
  calls.length = 0;
  loaded = true;
  total = 2;
  window.location.hash = '';
});

test('signed in with conversations: the timeline is read behind the modal, then the Calendar', async () => {
  await flow.enterSignedIn('');
  assert.deepEqual(calls, [['load', 'tok']]);
  assert.equal(page.el('loadingModal').hidden, true);
  assert.equal(page.el('mainContent').hidden, false);
  assert.equal(window.location.hash, '#calendar');
  assert.equal(flow.hasData(), true);
});

test('signed in with none: the Upload page; an address naming a tab opens it', async () => {
  total = 0;
  await flow.enterSignedIn('');
  assert.deepEqual(calls, [['resetUpload', false]]);
  total = 2;
  calls.length = 0;
  window.location.hash = '#review';
  await flow.enterSignedIn('#review');
  assert.deepEqual(calls, [['load', 'tok'], ['hash', '#review']]);
});

test('a timeline that fails to load, or holds nothing, says so in the modal and offers Try again', async () => {
  loaded = new Error('reading your sessions failed (500): boom');
  const error = console.error;
  console.error = () => {};
  await flow.openTimeline();
  assert.equal(page.el('loadingModal').hidden, false);
  assert.equal(page.el('loadingModalActions').hidden, false);
  assert.match(page.el('loadStatus').textContent, /reading your sessions failed \(500\): boom/);
  loaded = false;
  await flow.openTimeline();
  assert.match(page.el('loadStatus').textContent, /your timeline has no conversations/);
  console.error = error;
  loaded = true;
  await page.el('loadingRetryBtn').onclick();
  assert.equal(page.el('loadingModal').hidden, true);
});

test('after an upload, Describe opens; leaving it reads the timeline', async () => {
  flow.afterUpload({ files: [{ name: 'a.json' }, { name: 'b.json' }], processed: [{ index: 0, uploadId: 'u1' }], failed: [{ index: 1, error: new Error('bad') }], stopped: [] });
  assert.deepEqual(calls, [['describe', 'batch']]);
  await flow.leaveDescribe({ kind: 'batch' }, true);
  assert.deepEqual(calls.slice(1), [['load', 'tok']]);
});

test("saved details read the timeline again, then return to the file's or the conversation's tab", async () => {
  await flow.leaveDescribe({ kind: 'file', uploadId: 'u1' }, true);
  assert.deepEqual(calls, [['load', 'tok']]);
  assert.equal(window.location.hash, '#files');
  calls.length = 0;
  state.conversations = [{ id: 'c0', name: 'One', total_messages: 0 }];
  await flow.leaveDescribe({ kind: 'conversation', conversationId: 'c0' }, false);
  assert.deepEqual(calls, [], 'cancelled: nothing read');
  assert.equal(window.location.hash, '#conversations/0');
  loaded = new Error('nope');
  const error = console.error;
  console.error = () => {};
  await flow.leaveDescribe({ kind: 'conversation', conversationId: 'c0' }, true);
  console.error = error;
  assert.equal(page.el('loadingModal').hidden, true, 'a failed reading back closes the modal');
  // Plan §12.4: the read is of the timeline, not of your files, so the
  // words say what can be done about it.
  assert.equal(page.el('describeStatus').textContent, 'Could not complete request, please try again.');
  assert.equal(window.location.hash, '#conversations/0', 'and stays where it was');
  loaded = new PageError('reading your sessions failed (500): stored data can\'t be read', 'data_integrity', 500);
  console.error = () => {};
  await flow.leaveDescribe({ kind: 'conversation', conversationId: 'c0' }, true);
  console.error = error;
  assert.equal(page.el('describeStatus').textContent, 'Data integrity failure');
});

// Plan §12.4: the loading box says "Data integrity failure" when the
// server's stored data can't be read, and "Could not complete request,
// please try again." for a failure trying again may cure.
test('the loading box names damaged data, and asks to try again otherwise', async () => {
  const error = console.error;
  console.error = () => {};
  loaded = new PageError('reading your sessions failed (500): stored data can\'t be read', 'data_integrity', 500);
  await flow.openTimeline();
  assert.equal(page.el('loadStatus').textContent, 'Data integrity failure');
  for(const failure of [new PageError('reading your sessions failed (503)', 'server_error', 503), new TypeError('Failed to fetch')]){
    loaded = failure;
    await flow.openTimeline();
    assert.equal(page.el('loadStatus').textContent, 'Could not complete request, please try again.');
  }
  console.error = error;
  loaded = true;
  await page.el('loadingRetryBtn').onclick();
  assert.equal(page.el('loadingModal').hidden, true);
});
