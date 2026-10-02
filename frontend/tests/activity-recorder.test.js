// The activity recorder's sending rules (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, "When events are
// sent"), with a fake clock, a fake request tracker and a fake sender.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  BATCH_EVENTS, EARLY_SEND_COUNT, KEEPALIVE_BUDGET_BYTES, MAX_EVENT_BYTES, MAX_KEPT, MIN_INTERVAL_MS, QUIET_MS,
  createActivityRecorder,
} from '../infra/activity-recorder.js';
import { createRequestTracker } from '../core/request-tracker.js';

// Everything the recorder touches, faked. `answers` is what the fake sender
// replies, one per post, defaulting to 204; an Error is thrown instead.
function harness({ token = 'tok', answers = [] } = {}){
  const h = {
    clock: 1_000_000,
    token,
    tab: 'review',
    posts: [],
    warnings: [],
    deferred: [],
    answers: [...answers],
  };
  h.tracker = createRequestTracker(() => h.clock);
  h.recorder = createActivityRecorder({
    now: () => h.clock,
    tracker: h.tracker,
    token: () => h.token,
    post: async (body, options) => {
      h.posts.push({ body: JSON.parse(body), raw: body, ...options });
      const answer = h.answers.length ? h.answers.shift() : { ok: true, status: 204 };
      if(answer instanceof Error) throw answer;
      return answer;
    },
    defer: (fn) => h.deferred.push(fn),
    currentTab: () => h.tab,
    warn: (message) => h.warnings.push(message),
  });
  // Runs what the recorder deferred, and lets its sends settle.
  h.settle = async () => {
    while(h.deferred.length) await h.deferred.shift()();
    for(let i = 0; i < 5; i++) await Promise.resolve();
  };
  h.click = (n = 1) => {
    for(let i = 0; i < n; i++) h.recorder.record({ kind: 'click', target: { tag: 'button', id: `b${i}` } });
  };
  return h;
}

function on(options){
  const h = harness(options);
  h.recorder.decide(true);
  return h;
}

test('records are stamped with the time and the tab; a request keeps its own start time', async () => {
  const h = on();
  h.recorder.record({ kind: 'click', target: { tag: 'button' } });
  h.recorder.record({ kind: 'request', t: 5, method: 'GET', route: '/export' });
  await h.settle();
  assert.equal(h.posts.length, 1);
  assert.deepEqual(h.posts[0].body.events, [
    { kind: 'click', target: { tag: 'button' }, t: 1_000_000, tab: 'review' },
    { kind: 'request', t: 5, method: 'GET', route: '/export', tab: 'review' },
  ]);
  assert.equal(h.posts[0].token, 'tok');
  assert.equal(h.posts[0].keepalive, false);
  assert.equal('dropped' in h.posts[0].body, false);
});

test('nothing is sent while a page request is in flight, or within 3 s after it', async () => {
  const h = on();
  h.tracker.start();
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 0, 'in flight');

  h.tracker.finish();
  h.clock += QUIET_MS - 1;
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 0, 'within 3 s after');

  h.clock += 1;
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 1, 'quiet for 3 s');
  assert.equal(h.posts[0].body.events.length, 3);
  assert.equal(h.recorder.pending(), 0);
});

test('a request starting between the decision to send and the send itself holds it back', async () => {
  const h = on();
  h.click();
  h.tracker.start();
  await h.settle();
  assert.equal(h.posts.length, 0);
  assert.equal(h.recorder.pending(), 1);
});

test('at most one send a minute', async () => {
  const h = on();
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 1);

  h.clock += MIN_INTERVAL_MS - 1;
  h.click(10);
  await h.settle();
  assert.equal(h.posts.length, 1, 'less than a minute later');
  assert.equal(h.recorder.pending(), 10);

  h.clock += 1;
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 2, 'a minute later');
  assert.equal(h.posts[1].body.events.length, 11);
});

test('an early send at 500 waiting records, in batches of at most 200', async () => {
  const h = on();
  h.click();
  await h.settle();
  h.clock += 1000;
  h.click(EARLY_SEND_COUNT - 1);
  await h.settle();
  assert.equal(h.posts.length, 1, '499 waiting: not yet');
  h.click();
  await h.settle();
  assert.deepEqual(h.posts.slice(1).map((p) => p.body.events.length), [BATCH_EVENTS, BATCH_EVENTS, 100]);
  assert.equal(h.recorder.pending(), 0);
});

test('a page left idle sends nothing: sending is only considered when something is recorded', async () => {
  const h = on();
  h.tracker.start();
  h.click(3);
  h.tracker.finish();
  h.clock += 10 * MIN_INTERVAL_MS;
  await h.settle();
  assert.equal(h.posts.length, 0);
  assert.equal(h.deferred.length, 0, 'no send waiting to happen');
  assert.equal(h.recorder.pending(), 3);
});

test('records wait for a sign-in token', async () => {
  const h = on({ token: null });
  h.click(2);
  await h.settle();
  h.recorder.leave();
  await h.settle();
  assert.equal(h.posts.length, 0);
  h.token = 'later';
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 1);
  assert.equal(h.posts[0].body.events.length, 3);
  assert.equal(h.posts[0].token, 'later');
});

test('nothing is sent before recording is decided, and nothing at all when it is off', async () => {
  const h = harness();
  h.click(2);
  await h.settle();
  h.recorder.leave();
  assert.equal(h.posts.length, 0, 'undecided');
  assert.equal(h.recorder.pending(), 2);

  h.recorder.decide(false);
  assert.equal(h.recorder.pending(), 0);
  h.click(5);
  h.recorder.leave();
  await h.settle();
  assert.equal(h.posts.length, 0);
  assert.equal(h.recorder.pending(), 0);
});

test('records from before the decision are sent once it is on', async () => {
  const h = harness();
  h.click(2);
  h.recorder.decide(true);
  h.click();
  await h.settle();
  assert.equal(h.posts[0].body.events.length, 3);
});

test('a send on pagehide: everything waiting, with keepalive, ignoring the quiet and once-a-minute rules', async () => {
  const h = on();
  h.click();
  await h.settle();
  h.tracker.start();
  h.click(4);
  await h.settle();
  assert.equal(h.posts.length, 1);

  h.recorder.leave();
  await h.settle();
  assert.equal(h.posts.length, 2);
  assert.equal(h.posts[1].keepalive, true);
  assert.equal(h.posts[1].body.events.length, 4);
  assert.equal(h.recorder.pending(), 0);

  h.recorder.leave();
  assert.equal(h.posts.length, 2, 'nothing waiting: nothing sent');
});

test('leaving sends only what fits the keepalive allowance', async () => {
  const h = harness();
  const big = 'x'.repeat(3000);
  for(let i = 0; i < 40; i++) h.recorder.record({ kind: 'shown', where: 'error', text: big });
  h.recorder.decide(true);
  h.recorder.leave();
  await h.settle();
  const sent = h.posts.reduce((n, p) => n + p.body.events.length, 0);
  const bytes = h.posts.reduce((n, p) => n + p.raw.length, 0);
  assert.ok(bytes <= KEEPALIVE_BUDGET_BYTES, `${bytes} bytes`);
  assert.ok(sent > 0 && sent < 40, `${sent} sent`);
  assert.equal(h.recorder.pending(), 40 - sent);
});

test('at most 2,000 records are kept; the oldest go and the dropped count is sent', async () => {
  const h = on({ token: null });
  for(let i = 0; i < MAX_KEPT + 25; i++) h.recorder.record({ kind: 'click', target: { tag: 'button', id: `n${i}` } });
  assert.equal(h.recorder.pending(), MAX_KEPT);
  assert.equal(h.recorder.droppedCount(), 25);

  h.token = 'tok';
  h.click();
  await h.settle();
  assert.equal(h.posts[0].body.dropped, 26);
  assert.equal(h.posts[0].body.events[0].target.id, 'n26', 'the oldest were dropped');
  assert.equal('dropped' in h.posts[1].body, false, 'reported once');
  assert.equal(h.recorder.droppedCount(), 0);
});

test('a failed send warns with the status, keeps the records, and retries a minute later', async () => {
  const h = on({ answers: [{ ok: false, status: 503 }] });
  h.click(3);
  await h.settle();
  assert.equal(h.posts.length, 1);
  assert.match(h.warnings[0], /status 503/);
  assert.equal(h.recorder.pending(), 3);

  h.clock += MIN_INTERVAL_MS;
  h.click();
  await h.settle();
  assert.equal(h.posts.length, 2);
  assert.equal(h.posts[1].body.events.length, 4);
  assert.equal(h.posts[1].body.events[0].target.id, 'b0', 'kept in order');
});

test('a send that gets no answer warns with the error and keeps the records', async () => {
  const h = on({ answers: [new TypeError('Failed to fetch')] });
  h.click(2);
  await h.settle();
  assert.match(h.warnings[0], /no answer: Failed to fetch/);
  assert.equal(h.recorder.pending(), 2);
});

test('failed sends never keep more than 2,000 records', async () => {
  const h = on({ answers: [{ ok: false, status: 500 }] });
  h.click(BATCH_EVENTS);
  h.deferred.length = 0; // the quiet-moment send isn't run in this test
  h.recorder.leave();    // the batch is out; its answer comes later
  assert.equal(h.recorder.pending(), 0);
  h.click(MAX_KEPT);
  h.deferred.length = 0;
  for(let i = 0; i < 10; i++) await Promise.resolve();
  assert.equal(h.posts.length, 1);
  assert.equal(h.recorder.pending(), MAX_KEPT);
  assert.equal(h.recorder.droppedCount(), BATCH_EVENTS);
});

test('a batch the route refuses (400) is dropped and counted, not retried', async () => {
  const h = on({ answers: [{ ok: false, status: 400 }] });
  h.click(2);
  await h.settle();
  assert.match(h.warnings[0], /refused \(status 400\); 2 dropped/);
  assert.equal(h.recorder.pending(), 0);
  assert.equal(h.recorder.droppedCount(), 2);

  h.clock += MIN_INTERVAL_MS;
  h.click();
  await h.settle();
  assert.equal(h.posts[1].body.dropped, 2);
  assert.equal(h.recorder.droppedCount(), 0);
});

test('a record too large for the route is dropped, counted and reported', async () => {
  const h = on();
  h.recorder.record({ kind: 'shown', where: 'error', text: 'x'.repeat(MAX_EVENT_BYTES) });
  await h.settle();
  assert.equal(h.posts.length, 0, 'nothing else to send');
  assert.match(h.warnings[0], /over the 4096-byte limit/);
  assert.equal(h.recorder.droppedCount(), 1);
});

test('the request tracker counts requests in flight and never goes below zero', () => {
  let clock = 7;
  const tracker = createRequestTracker(() => clock);
  assert.equal(tracker.lastFinishedAt(), null);
  tracker.start();
  tracker.start();
  assert.equal(tracker.inFlight(), 2);
  clock = 9;
  tracker.finish();
  tracker.finish();
  tracker.finish();
  assert.equal(tracker.inFlight(), 0);
  assert.equal(tracker.lastFinishedAt(), 9);
});
