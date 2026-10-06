// The progress bars and their clock (ui/widgets/status-indicators.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b): the bar
// moves only on real progress, every measured bar clears the striped
// state, a clock worded as time spent ticks beside the words, and the
// activity log records the words once, not each tick. Node has no page,
// so the bar's elements are stand-ins, and so is the clock.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { connectActivitySink } from '../core/activity-sink.js';

function element(){
  const classes = new Set();
  return {
    hidden: true, textContent: '', style: {},
    classList: { add: (c) => classes.add(c), remove: (...cs) => cs.forEach((c) => classes.delete(c)), has: (c) => classes.has(c) },
  };
}
const els = { loadProgress: element(), loadProgressFill: element(), loadProgressLabel: element() };
globalThis.document = { getElementById: (id) => els[id] };

const bars = await import('../ui/widgets/status-indicators.js');

function fakeTimers(){
  const t = { now: 0, every: null, cancelled: 0 };
  return Object.assign(t, {
    timers: {
      now: () => t.now,
      every: (fn) => { t.every = fn; return 'handle'; },
      cancel: (h) => { assert.equal(h, 'handle'); t.cancelled += 1; t.every = null; },
    },
    tick(ms){ t.now += ms; if(t.every) t.every(); },
  });
}

function recording(){
  const events = [];
  connectActivitySink({ record: (e) => events.push(e), requestStarted(){}, requestFinished(){} });
  return events;
}

test('a wait that measures nothing is striped, with a clock worded as time spent', () => {
  const fill = element(), label = element();
  const t = fakeTimers();
  const events = recording();
  const bar = bars.createProgressBar({ nodes: () => ({ fill, label }), where: 'loadProgress' }, t.timers);
  bar.working('progress.signing_in');
  assert.ok(fill.classList.has('is-working'));
  assert.equal(fill.style.width, '100%');
  assert.equal(label.textContent, 'Signing in…');
  t.tick(999);
  assert.equal(label.textContent, 'Signing in…');
  t.tick(1);
  assert.equal(label.textContent, 'Signing in… · 1 s so far');
  t.tick(11000);
  assert.equal(label.textContent, 'Signing in… · 12 s so far');
  connectActivitySink(null);
  assert.deepEqual(events.map((e) => e.message), ['progress.signing_in'], 'recorded once, not each tick');
});

test('real progress clears the stripes, fills the bar, and keeps the wait\'s clock', () => {
  const fill = element(), label = element();
  const t = fakeTimers();
  const events = recording();
  const bar = bars.createProgressBar({ nodes: () => ({ fill, label }), where: 'loadProgress' }, t.timers);
  bar.working('progress.scan_starting');
  t.tick(2000);
  bar.measured('progress.scanning', {}, 41, 117);
  assert.equal(fill.classList.has('is-working'), false);
  assert.equal(fill.style.width, '35%');
  assert.equal(label.textContent, 'Scanning your messages — 41 of 117 sessions (35%) · 2 s so far');
  bar.measured('progress.scanning', {}, 117, 117);
  bar.measured('progress.scanning', {}, 1, 0);
  assert.equal(fill.style.width, '100%');
  bar.fillTo(500, 100);
  assert.equal(fill.style.width, '100%', 'never past full');
  connectActivitySink(null);
  assert.deepEqual(events.map((e) => e.message), ['progress.scan_starting', 'progress.scanning']);
});

test('words with their own clock, a failure, and a reset stop the bar\'s clock', () => {
  const fill = element(), label = element();
  const t = fakeTimers();
  const bar = bars.createProgressBar({ nodes: () => ({ fill, label }) }, t.timers);
  bar.working('progress.processing');
  bar.label('wait.processing', { answer: { attempt: 1 }, elapsedMs: 5000 });
  assert.equal(t.cancelled, 1);
  t.tick(3000);
  assert.equal(label.textContent, 'Processing on the server — 5s');
  bar.measured('progress.sessions', {}, 1, 4);
  bar.failed('progress.request_failed', { detail: 'boom' });
  assert.ok(fill.classList.has('is-error'));
  assert.equal(fill.style.width, '25%');
  assert.equal(label.textContent, 'Could not finish: boom');
  bar.reset();
  assert.deepEqual([fill.style.width, label.textContent, fill.classList.has('is-error')], ['0%', '', false]);
  bar.failed('progress.request_failed', { detail: 'again' });
  assert.equal(fill.style.width, '100%', 'a failure before any progress fills the bar red');
  bar.stop();
});

test("the load screen's bar: shown, striped, measured for each kind of progress, then hidden", () => {
  const events = recording();
  bars.showLoadProgress();
  assert.equal(els.loadProgress.hidden, false);
  assert.equal(els.loadProgressFill.style.width, '0%');
  bars.setLoadProgressIndeterminate('progress.reading_file');
  assert.ok(els.loadProgressFill.classList.has('is-working'));
  bars.showPrepareProgress({ read: 1024, size: 4096, conversations: 2, compressed: 300 });
  assert.equal(els.loadProgressFill.style.width, '25%');
  assert.match(els.loadProgressLabel.textContent, /^Preparing the file — 1 KB of 4 KB read, 2 conversations slimmed, 300 B compressed/);
  bars.showSendProgress(512, 1024, 'almost done');
  assert.match(els.loadProgressLabel.textContent, /^Sending your file — 512 B of 1 KB \(50%\) — almost done/);
  bars.showSendProgress(1024, 1024, '');
  assert.match(els.loadProgressLabel.textContent, /^Sending your file — 1 KB of 1 KB \(100%\)/);
  bars.showWaitProgress({ status: 'processing' }, 2000);
  assert.equal(els.loadProgressLabel.textContent, 'Waiting for the server to start — 2s');
  bars.showWaitProgress({ status: 'processing', attempt: 1, progress: {
    bytes_read: 3, bytes_total: 4, conversations_written: 0, conversations_total: 0,
  } }, 3000);
  assert.equal(els.loadProgressFill.style.width, '75%');
  assert.equal(els.loadProgressLabel.textContent, 'Processing on the server — 3s · reading your file: 3 B of 4 B');
  bars.showScanProgress(1, 2);
  assert.match(els.loadProgressLabel.textContent, /^Scanning your messages — 1 of 2 sessions \(50%\)/);
  bars.failLoadProgress();
  assert.ok(els.loadProgressFill.classList.has('is-error'));
  bars.hideLoadProgress();
  assert.equal(els.loadProgress.hidden, true);
  bars.failLoadProgress();
  connectActivitySink(null);
  assert.equal(events.filter((e) => e.message === 'progress.failed').length, 1, 'a hidden bar is not failed again');
});
