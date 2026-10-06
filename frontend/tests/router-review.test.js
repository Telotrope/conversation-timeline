// An address naming the Review tab asks for its results if they aren't
// asked for yet (ui/router.js, ui/views/review.js's showReviewTab; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '#review' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';
page.el('reviewFilter').value = 'all';

const asked = [];
globalThis.fetch = async (url) => {
  const { pathname } = new URL(url);
  asked.push(pathname);
  const body = pathname === '/_dev/login' ? { token: 'tok' }
    : { rows: [], matched: 0, notes: 0, page_starts: [], cursor: null, sessions_done: 0, sessions_total: 0, data_version: 1 };
  return { ok: true, status: 200, headers: new Headers(), json: async () => body };
};

const { applyLocationHash } = await import('../ui/router.js');

test('#review shows Review and asks for its results', async () => {
  applyLocationHash();
  for(let i = 0; i < 10; i++) await new Promise((r) => setImmediate(r));
  assert.deepEqual(asked, ['/_dev/login', '/messages']);
});
