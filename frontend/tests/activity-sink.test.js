import { test } from 'node:test';
import assert from 'node:assert/strict';
import { connectActivitySink, noteRequestFinished, noteRequestStarted, recordActivity } from '../core/activity-sink.js';

test('with nothing connected, recording does nothing', () => {
  connectActivitySink(null);
  assert.doesNotThrow(() => { recordActivity({ kind: 'click' }); noteRequestStarted(); noteRequestFinished(); });
});

test('a connected sink receives records and request starts and finishes', () => {
  const calls = [];
  connectActivitySink({
    record: (e) => calls.push(['record', e.kind]),
    requestStarted: () => calls.push(['started']),
    requestFinished: () => calls.push(['finished']),
  });
  noteRequestStarted();
  recordActivity({ kind: 'request' });
  noteRequestFinished();
  connectActivitySink(null);
  recordActivity({ kind: 'click' });
  assert.deepEqual(calls, [['started'], ['record', 'request'], ['finished']]);
});

test('a sink that throws is reported in the console, never thrown into the page', (t) => {
  const warned = [];
  t.mock.method(console, 'warn', (m) => warned.push(m));
  connectActivitySink({ record(){ throw new Error('full'); } });
  assert.doesNotThrow(() => recordActivity({ kind: 'click' }));
  connectActivitySink(null);
  assert.deepEqual(warned, ['activity recording failed (record): full']);
});
