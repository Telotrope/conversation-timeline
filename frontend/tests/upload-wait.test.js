import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  waitForProcessing, FAST_INTERVAL_MS, SLOW_INTERVAL_MS, GIVE_UP_AFTER_MS,
} from '../core/upload-wait.js';

// A pretend clock that only moves when the code sleeps, and a server that
// gives the listed answers in turn (repeating the last one).
function run(answers){
  let time = 0;
  const sleeps = [];
  let asked = 0;
  const promise = waitForProcessing({
    fetchStatus: async () => {
      const answer = answers[Math.min(asked, answers.length - 1)];
      asked += 1;
      if(answer instanceof Error) throw answer;
      return answer;
    },
    sleep: async (ms) => { sleeps.push(ms); time += ms; },
    now: () => time,
  });
  return { promise, sleeps, asked: () => asked };
}

const processing = { status: 'processing' };
const ready = { status: 'ready' };

test('ready on the first answer: nothing waits', async () => {
  const r = run([ready]);
  await r.promise;
  assert.deepEqual(r.sleeps, []);
});

test('asks every second at first, then every five seconds, until ready', async () => {
  const r = run([...Array(13).fill(processing), ready]);
  await r.promise;
  assert.equal(r.asked(), 14);
  assert.deepEqual(r.sleeps, [...Array(10).fill(FAST_INTERVAL_MS), ...Array(3).fill(SLOW_INTERVAL_MS)]);
});

test("a failed upload rejects with the server's reason", async () => {
  const r = run([processing, { status: 'failed', reason: 'not an export' }]);
  await assert.rejects(r.promise, /couldn't process the file: not an export/);
});

test('gives up after ten minutes of "processing"', async () => {
  const r = run([processing]);
  await assert.rejects(
    r.promise,
    /^Error: No answer from the server after 10 minutes\. Its last status was: Waiting for the server to start — 10:00\. Reload later to check again\.$/,
  );
  assert.ok(r.sleeps.reduce((a, b) => a + b, 0) >= GIVE_UP_AFTER_MS);
});

test('a failed request is passed on unchanged', async () => {
  const offline = new TypeError('Failed to fetch');
  const r = run([processing, offline]);
  await assert.rejects(r.promise, (e) => e === offline);
});

test('an unknown status is an error naming it', async () => {
  const r = run([{ status: 'queued' }]);
  await assert.rejects(r.promise, /unknown upload status: "queued"/);
});
