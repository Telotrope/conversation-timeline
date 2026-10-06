// The page's side of preparing a file (infra/upload-preparer.js): one Web
// Worker per file, its progress passed on, its answer turned into what is
// sent, and every way it can fail or be stopped. Node has no Web Worker, so
// a stand-in class records what the page does with it and answers as the
// worker would.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { fileURLToPath } from 'node:url';
import { WORKER_URL, prepareUpload } from '../infra/upload-preparer.js';

class FakeWorker {
  static last = null;
  constructor(url, options){
    this.url = url;
    this.options = options;
    this.listeners = {};
    this.terminated = false;
    FakeWorker.last = this;
  }
  addEventListener(type, fn){ this.listeners[type] = fn; }
  postMessage(message){ this.sent = message; }
  terminate(){ this.terminated = true; }
  // As the worker would: an event of `type`.
  fire(type, event){ this.listeners[type](event); }
}
globalThis.Worker = FakeWorker;

const file = { name: 'conversations.json', size: 10 };

test('the worker is the module file next to the page code, started as a module', () => {
  prepareUpload(file, () => {});
  assert.equal(FakeWorker.last.url, WORKER_URL);
  assert.deepEqual(FakeWorker.last.options, { type: 'module' });
  assert.deepEqual(FakeWorker.last.sent, { file });
  assert.ok(fs.existsSync(fileURLToPath(WORKER_URL)), `${WORKER_URL} exists`);
});

test('progress is passed on, and a slimmed file is sent compressed', async () => {
  const seen = [];
  const prepared = prepareUpload(file, (p) => seen.push(p.read));
  const worker = FakeWorker.last;
  worker.fire('message', { data: { kind: 'progress', read: 4 } });
  const blob = new Blob(['x']);
  worker.fire('message', { data: { kind: 'done', blob, conversations: 3, slimmed: 99 } });
  assert.deepEqual(await prepared, { body: blob, slimmed: true, conversations: 3, slimmedBytes: 99 });
  assert.deepEqual(seen, [4]);
  assert.equal(worker.terminated, true);
});

test('a file the worker cannot slim is sent as it is', async () => {
  const prepared = prepareUpload(file, () => {});
  FakeWorker.last.fire('message', { data: { kind: 'not_export', reason: 'the file is empty' } });
  assert.deepEqual(await prepared, { body: file, slimmed: false, reason: 'the file is empty' });
});

test('a worker that fails, cannot start, or answers unreadably fails the file, saying why', async () => {
  let prepared = prepareUpload(file, () => {});
  FakeWorker.last.fire('message', { data: { kind: 'failed', reason: 'TypeError: the disk went away' } });
  await assert.rejects(prepared, /preparing the file failed: TypeError: the disk went away/);
  prepared = prepareUpload(file, () => {});
  FakeWorker.last.fire('error', { message: 'SyntaxError in the worker' });
  await assert.rejects(prepared, /SyntaxError in the worker/);
  prepared = prepareUpload(file, () => {});
  FakeWorker.last.fire('error', {});
  await assert.rejects(prepared, /the worker could not start/);
  prepared = prepareUpload(file, () => {});
  FakeWorker.last.fire('messageerror', {});
  await assert.rejects(prepared, /answer could not be read/);
  assert.equal(FakeWorker.last.terminated, true);
});

test('Stop ends the worker and the preparation', async () => {
  let stop = null;
  const prepared = prepareUpload(file, () => {}, (fn) => { stop = fn; });
  stop();
  await assert.rejects(prepared, (err) => err.kind === 'aborted' && /stopped/.test(err.message));
  assert.equal(FakeWorker.last.terminated, true);
});
