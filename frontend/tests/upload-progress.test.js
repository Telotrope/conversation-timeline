// How far the server has got with an upload, from the progress its
// "processing" answers carry (core/upload-wait.js's processingProgress; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b), and its
// words under the bar.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { processingProgress } from '../core/upload-wait.js';
import { pageMessage } from '../ui/widgets/page-messages.js';

const progress = (read, total, written, count) => ({
  bytes_read: read, bytes_total: total, conversations_written: written, conversations_total: count,
});

test('bytes read first, then conversations written', () => {
  assert.deepEqual(processingProgress(progress(1024, 4096, 0, 0)), { text: 'reading your file: 1 KB of 4 KB', done: 1024, total: 4096 });
  assert.deepEqual(processingProgress(progress(4096, 4096, 0, 12)), { text: 'storing your conversations: 0 of 12', done: 0, total: 12 });
  assert.deepEqual(processingProgress(progress(4096, 4096, 5, 12)), { text: 'storing your conversations: 5 of 12', done: 5, total: 12 });
});

test('no progress, or totals not yet known, measure nothing', () => {
  assert.equal(processingProgress(undefined), null);
  assert.equal(processingProgress(progress(0, 0, 0, 0)), null);
  assert.equal(processingProgress(progress(10, 10, 0, 0)), null);
});

test('the wait line adds the progress after its clock', () => {
  const answer = { status: 'processing', attempt: 1, max_attempts: 3, progress: progress(4096, 4096, 5, 12) };
  assert.equal(pageMessage('wait.processing').text({ answer, elapsedMs: 3000 }),
    'Processing on the server — 3s · storing your conversations: 5 of 12');
});
