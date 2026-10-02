import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PageError, errorKindOf, errorStatusOf } from '../core/page-error.js';

test('the kind of each sort of failure', () => {
  assert.equal(errorKindOf(new PageError('x', 'stale_page', 403)), 'stale_page');
  assert.equal(errorKindOf(new PageError('x', 'not a kind')), 'other');
  assert.equal(errorKindOf(new TypeError('Failed to fetch')), 'network');
  const aborted = new Error('stop');
  aborted.name = 'AbortError';
  assert.equal(errorKindOf(aborted), 'aborted');
  assert.equal(errorKindOf(new Error('anything')), 'other');
  assert.equal(errorKindOf(null), 'other');
});

test('a PageError reads as a plain Error, with its status', () => {
  const e = new PageError('starting the upload failed (500)', 'server_error', 500);
  assert.equal(String(e), 'Error: starting the upload failed (500)');
  assert.equal(errorStatusOf(e), 500);
  assert.equal(errorStatusOf(new PageError('x', 'network')), null);
  assert.equal(errorStatusOf(new Error('x')), null);
});
