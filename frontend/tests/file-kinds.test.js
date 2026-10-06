// How each kind of stored file is shown (core/file-kinds.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fileView } from '../core/file-kinds.js';

test('each kind has its way of being shown, none of which runs the file', () => {
  assert.deepEqual(fileView({ kind: 'svg' }), { mode: 'image', mime: 'image/svg+xml', label: 'drawing', language: null });
  assert.deepEqual(fileView({ kind: 'web_page' }), { mode: 'frame', mime: 'text/html', label: 'web page', language: null });
  assert.equal(fileView({ kind: 'markdown' }).mode, 'markdown');
  assert.deepEqual(fileView({ kind: 'code', language: 'python' }), { mode: 'code', mime: 'text/plain', label: 'python code', language: 'python' });
  assert.deepEqual(fileView({ kind: 'code' }), { mode: 'code', mime: 'text/plain', label: 'code', language: null });
  assert.equal(fileView({ kind: 'text' }).mode, 'text');
});

test("kinds the page doesn't know are shown as text", () => {
  assert.deepEqual(fileView({ kind: 'other' }), { mode: 'text', mime: 'text/plain', label: 'file', language: null });
  assert.equal(fileView({ kind: 'toString' }).mode, 'text');
});
