// Several files at once (core/upload-batch.js; plan
// docs/plans/2026-10-05-screen-flow.md §6): combined progress, one failure
// leaving the rest, and Stop.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { StoppedError, startBatch } from '../core/upload-batch.js';

const files = [{ name: 'a.json', size: 100 }, { name: 'b.json', size: 300 }];

function recorder(){
  const progress = [];
  const failed = [];
  return { progress, failed, on: { progress: (l, t) => progress.push([l, t]), failed: (i, e) => failed.push([i, e.message]) } };
}

test('progress adds up across files, and every file processed is reported', async () => {
  const r = recorder();
  const batch = startBatch(files, async (file, ctx) => {
    ctx.sent(file.size / 2);
    ctx.sendingDone();
    return `id-${file.name}`;
  }, r.on);
  const result = await batch.done;
  assert.deepEqual(result, {
    processed: [{ index: 0, uploadId: 'id-a.json' }, { index: 1, uploadId: 'id-b.json' }],
    failed: [], stopped: [],
  });
  assert.deepEqual(r.progress.at(-1), [400, 400]);
  assert.ok(r.progress.some(([loaded]) => loaded === 50));
});

test('one file failing leaves the others running', async () => {
  const r = recorder();
  const batch = startBatch(files, async (file) => {
    if(file.name === 'a.json') throw new Error('refused');
    return 'id-b';
  }, r.on);
  const result = await batch.done;
  assert.deepEqual(result.processed, [{ index: 1, uploadId: 'id-b' }]);
  assert.equal(result.failed[0].index, 0);
  assert.deepEqual(r.failed, [[0, 'refused']]);
});

test('Stop cancels sends, ends waits, and counts those files as stopped', async () => {
  const r = recorder();
  let aborted = 0;
  let release;
  const sending = new Promise((resolve) => { release = resolve; });
  const batch = startBatch(files, async (file, ctx) => {
    if(file.name === 'a.json'){
      ctx.registerAbort(() => { aborted += 1; release(); });
      await sending;
      throw new Error('aborted');
    }
    await ctx.sleep(60_000);
    return 'never';
  }, r.on);
  batch.stop();
  const result = await batch.done;
  assert.deepEqual(result, { processed: [], failed: [], stopped: [0, 1] });
  assert.equal(aborted, 1);
  assert.deepEqual(r.failed, []);
});

test('after Stop, a send that starts is cancelled at once and a pause fails at once', async () => {
  const r = recorder();
  let gate;
  const opened = new Promise((resolve) => { gate = resolve; });
  const events = [];
  const batch = startBatch([files[0]], async (file, ctx) => {
    await opened;
    ctx.registerAbort(() => events.push('abort'));
    await ctx.sleep(10).catch((e) => { events.push(e instanceof StoppedError ? 'stopped' : 'other'); throw e; });
    return 'id';
  }, r.on);
  batch.stop();
  gate();
  const result = await batch.done;
  assert.deepEqual(events, ['abort', 'stopped']);
  assert.deepEqual(result.stopped, [0]);
});

test('a pause that runs its course resolves', async () => {
  const r = recorder();
  const batch = startBatch([files[0]], async (file, ctx) => { await ctx.sleep(1); return 'id'; }, r.on);
  assert.deepEqual((await batch.done).processed, [{ index: 0, uploadId: 'id' }]);
  assert.equal(new StoppedError().message, 'stopped');
});
