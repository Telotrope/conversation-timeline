// The line under the bar while the server processes an upload, and the hook
// that feeds it each answer (plan 2026-10-02-upload-processing-failures.md
// §3): which attempt is running, why the last one failed, and a clock.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { describeWait, formatElapsed, waitForProcessing } from '../core/upload-wait.js';

test('the clock shows minutes and zero-padded seconds', () => {
  assert.equal(formatElapsed(0), '0:00');
  assert.equal(formatElapsed(9_999), '0:09');
  assert.equal(formatElapsed(94_000), '1:34');
  assert.equal(formatElapsed(600_000), '10:00');
});

test('before the first attempt: waiting for the server to start', () => {
  assert.equal(describeWait({ status: 'processing' }, 12_000), 'Waiting for the server to start — 0:12');
});

test('during a first attempt with no error: processing', () => {
  const answer = { status: 'processing', attempt: 1, max_attempts: 3 };
  assert.equal(describeWait(answer, 20_000), 'Processing on the server — 0:20');
});

test('after attempt 1 failed: names the error and says it will try again', () => {
  const answer = { status: 'processing', attempt: 1, max_attempts: 3, last_error: 'item not found' };
  assert.equal(
    describeWait(answer, 40_000),
    'The server hit an error (item not found) on attempt 1 of 3 and will try again automatically in 1–2 minutes. — 0:40',
  );
});

test('on a retry: names the error and the attempt', () => {
  const answer = { status: 'processing', attempt: 2, max_attempts: 3, last_error: 'item not found' };
  assert.equal(
    describeWait(answer, 94_000),
    'The server hit an error (item not found) and is trying again automatically: attempt 2 of 3. '
      + 'AWS waits 1–2 minutes between attempts. — 1:34',
  );
});

test('an error with no attempt counted still shows the error', () => {
  const answer = { status: 'processing', last_error: 'early' };
  assert.equal(describeWait(answer, 5_000), 'The server hit an error (early). — 0:05');
});

test('every answer is passed to onAnswer, including the last', async () => {
  const answers = [
    { status: 'processing' },
    { status: 'processing', attempt: 1, max_attempts: 3 },
    { status: 'ready' },
  ];
  const seen = [];
  let asked = 0;
  await waitForProcessing({
    fetchStatus: async () => answers[asked++],
    sleep: async () => {},
    now: () => 0,
    onAnswer: (a) => seen.push(a),
  });
  assert.deepEqual(seen, answers);
});
