// The Upload page's steps (ui/load-flow.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b, §8, §8b): each
// file prepared in a worker and sent slimmed (or as it is, when it isn't an
// export, or when the address asks), one bar across the batch, and the scan
// carried on with its cursor until done. The page, the worker and the
// upload request are stand-ins; `fetch` is a scripted server.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';

// The worker: answers the file it is posted as the test says, after
// `workerDelay(file)` ms (none unless a test says).
let workerAnswer = () => [];
let workerDelay = () => 0;
class FakeWorker {
  constructor(){ this.listeners = {}; }
  addEventListener(type, fn){ this.listeners[type] = fn; }
  postMessage({ file }){
    setTimeout(() => { for(const data of workerAnswer(file)) this.listeners.message({ data }); }, workerDelay(file));
  }
  terminate(){}
}
globalThis.Worker = FakeWorker;

// The upload request: reports half then all of what it sends.
const sent = [];
class FakeXhr {
  constructor(){ this.upload = {}; }
  open(method, url){ this.url = url; }
  send(body){
    sent.push({ url: this.url, body });
    setImmediate(() => {
      this.upload.onprogress({ lengthComputable: true, loaded: body.size / 2, total: body.size });
      this.status = 200;
      this.responseText = '';
      this.onload();
    });
  }
  abort(){ this.onabort(); }
}
globalThis.XMLHttpRequest = FakeXhr;

const asked = [];
let detectParts = [];
let conversationsTotal = 2;
globalThis.fetch = async (url, init = {}) => {
  const { pathname } = new URL(url);
  asked.push([init.method || 'GET', pathname, init.body]);
  const ok = (body) => ({ ok: true, status: 200, headers: new Headers(), json: async () => body });
  if(pathname === '/_dev/login') return ok({ token: 'tok' });
  if(pathname === '/uploads' && init.method === 'POST') return ok({ upload_id: `u${asked.length}`, upload_url: '/_dev/local-storage/put/raw' });
  if(pathname.startsWith('/uploads/')) return ok({ status: 'ready' });
  if(pathname === '/detect') return ok(detectParts.shift());
  if(pathname === '/conversations') return ok({ conversations: [], total: conversationsTotal, cursor: null, data_version: 1 });
  throw new Error(`nothing scripted for ${pathname}`);
};

const flow = await import('../ui/load-flow.js');
const { hideLoadProgress } = await import('../ui/widgets/status-indicators.js');

const results = [];
flow.connectUploadPage({
  hasData: () => false, dataArrived: () => {}, toSignIn: () => {}, afterUpload: (r) => results.push(r),
});

function choose(...files){
  page.el('loadConvFile').files = files;
  flow.chooseFiles();
}

beforeEach(() => {
  asked.length = 0;
  sent.length = 0;
  results.length = 0;
  detectParts = [];
  conversationsTotal = 2;
  workerDelay = () => 0;
  window.location.search = '';
  page.el('autoDetectCheckbox').checked = false;
  flow.resetUploadPage(false);
});

test('each file is prepared, sent slimmed, and processed; the bar goes from preparing to sending', async () => {
  const slim = new Blob(['gz']);
  workerAnswer = (file) => [
    { kind: 'progress', read: file.size / 2, size: file.size, conversations: 1, compressed: 1 },
    { kind: 'done', blob: slim, conversations: 2, slimmed: 9 },
  ];
  const labels = [];
  const label = page.el('loadProgressLabel');
  Object.defineProperty(label, 'textContent', { get(){ return this.text; }, set(v){ this.text = v; labels.push(v); }, configurable: true });
  choose(new File(['[{"uuid":"a"}]'], 'a.json'), new File(['[{"uuid":"b"}]'], 'b.json'));
  await flow.handleUploadClick();
  assert.deepEqual(sent.map((s) => s.body), [slim, slim]);
  assert.equal(sent[0].url, 'http://127.0.0.1:3000/_dev/local-storage/put/raw');
  assert.equal(results.length, 1);
  assert.deepEqual(results[0].processed.map((p) => p.index), [0, 1]);
  // Bytes read of both files' 28, then bytes sent of the 4 they slimmed to.
  const preparing = labels.findIndex((l) => /^Preparing the file — \d+ B of 28 B read, \d conversations? slimmed/.test(l));
  const sending = labels.findIndex((l) => /^Sending your file — \d B of 4 B/.test(l));
  assert.ok(preparing >= 0 && sending > preparing, labels.join('\n'));
  assert.equal(page.el('loadStatus').textContent, '');
  delete label.textContent;
});

test('a file the worker cannot slim, or any file when the address asks, is sent as it is', async () => {
  workerAnswer = () => [{ kind: 'not_export', reason: 'the file is empty' }];
  const file = new File(['not json'], 'x.json', { type: 'application/json' });
  choose(file);
  const info = console.info;
  console.info = () => {};
  await flow.handleUploadClick();
  console.info = info;
  assert.equal(sent[0].body, file);
  flow.resetUploadPage(false);
  sent.length = 0;
  window.location.search = '?upload=unslimmed';
  assert.equal(flow.sendsUnslimmed(), true);
  workerAnswer = () => { throw new Error('no worker should be started'); };
  choose(file);
  await flow.handleUploadClick();
  assert.equal(await sent[0].body.text(), 'not json');
  assert.equal(sent[0].body.type, '', 'sent with no type of its own');
});

test('a file whose preparation fails is listed with why; the others go on', async () => {
  workerAnswer = (file) => (file.name === 'bad.json'
    ? [{ kind: 'failed', reason: 'TypeError: the disk went away' }]
    : [{ kind: 'done', blob: new Blob(['gz']), conversations: 1, slimmed: 2 }]);
  choose(new File(['x'], 'bad.json'), new File(['[{}]'], 'good.json'));
  const error = console.error;
  console.error = () => {};
  await flow.handleUploadClick();
  console.error = error;
  assert.equal(results[0].failed[0].index, 0);
  assert.equal(sent.length, 1);
});

test('the scan is asked again with its cursor until done, the bar showing sessions done of the total', async () => {
  workerAnswer = () => [{ kind: 'done', blob: new Blob(['gz']), conversations: 1, slimmed: 2 }];
  page.el('autoDetectCheckbox').checked = true;
  detectParts = [
    { sessions_done: 2, sessions_total: 5, messages_detected: 7, cursor: 'd1', data_version: 3 },
    { sessions_done: 5, sessions_total: 5, messages_detected: 4, cursor: null, data_version: 4 },
  ];
  choose(new File(['[{}]'], 'a.json'));
  await flow.handleUploadClick();
  const scans = asked.filter(([, path]) => path === '/detect').map(([, , body]) => JSON.parse(body));
  assert.deepEqual(scans, [{}, { cursor: 'd1' }]);
  assert.equal(results.length, 1);
});

test('the scan stops when asked, and an upload of no conversations says so', async () => {
  let stopped = false;
  detectParts = [{ sessions_done: 1, sessions_total: 9, messages_detected: 0, cursor: 'd1', data_version: 1 }];
  const seen = [];
  assert.equal(await flow.runDetectionPass('tok', (done, total) => { seen.push([done, total]); stopped = true; }, () => stopped), 0);
  assert.deepEqual(seen, [[1, 9]]);
  workerAnswer = () => [{ kind: 'done', blob: new Blob(['gz']), conversations: 1, slimmed: 2 }];
  conversationsTotal = 0;
  choose(new File(['[{}]'], 'a.json'));
  await flow.handleUploadClick();
  assert.equal(results.length, 0);
  assert.match(page.el('loadStatus').textContent, /contained no conversations/);
  hideLoadProgress();
});

test('files are sent in their order, even when a later one is prepared first', async () => {
  workerAnswer = (file) => [{ kind: 'done', blob: new Blob([file.name]), conversations: 1, slimmed: 1 }];
  workerDelay = (file) => (file.name === 'slow.json' ? 30 : 0);
  choose(new File(['[{}]'], 'slow.json'), new File(['[{}]'], 'quick.json'));
  await flow.handleUploadClick();
  assert.deepEqual(await Promise.all(sent.map((s) => s.body.text())), ['slow.json', 'quick.json']);
  assert.deepEqual(asked.filter(([method, path]) => method === 'POST' && path === '/uploads').length, 2);
});
