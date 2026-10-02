// The tab and load-screen notices that keep the recorder from looking the
// tab up on every click (core/activity-sink.js; plan C18).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { connectActivitySink, noteMainShown, noteTabShown } from '../core/activity-sink.js';
import { waitMessageId } from '../core/upload-wait.js';

test('tab and load-screen changes reach the connected sink, and nothing when none is', () => {
  const calls = [];
  connectActivitySink({ tabShown: (n) => calls.push(['tab', n]), mainShown: (s) => calls.push(['main', s]) });
  noteTabShown('review');
  noteMainShown(true);
  connectActivitySink(null);
  noteTabShown('calendar');
  assert.deepEqual(calls, [['tab', 'review'], ['main', true]]);
});

test("which wait message each of the server's answers gets", () => {
  assert.equal(waitMessageId({ status: 'processing' }), 'wait.waiting');
  assert.equal(waitMessageId({ attempt: 1 }), 'wait.processing');
  assert.equal(waitMessageId({ last_error: 'x' }), 'wait.error');
  assert.equal(waitMessageId({ attempt: 1, last_error: 'x' }), 'wait.will_retry');
  assert.equal(waitMessageId({ attempt: 2, last_error: 'x' }), 'wait.retrying');
});
