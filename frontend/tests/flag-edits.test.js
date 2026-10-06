// Saving your flags (ui/flag-edits.js, ui/refresh-views.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §6, §8c): a box
// or Approve saves all three flags with the row's handle; the save answers
// with the session counted again, which replaces the page's copy, every
// view is drawn again from it, and Review asks for the same page again.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';
import { state } from '../core/state.js';
import { block, counts, resetState, session } from './fixtures.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
globalThis.requestAnimationFrame = (fn) => fn();
page.el('devLoginSub').value = 'alice';
page.el('reviewFilter').value = 'all';
page.el('view-review').classList.add('active');

const edits = await import('../ui/flag-edits.js');
const review = await import('../ui/views/review.js');

const asked = [];
let patchAnswer = null;
const messagesPart = {
  rows: [{
    kind: 'message', conversation_id: 'c0', message_id: 'm1', at: '2026-03-02T10:00:00Z', handle: 'h1',
    pieces: [], attachments: [], reply: null,
    flags: { auto: { caps: false, critical: true, angry: false }, user: { caps: null, critical: null, angry: null } },
  }],
  matched: 1, notes: 0, page_starts: [], cursor: null, sessions_done: 1, sessions_total: 1, data_version: 1,
};
globalThis.fetch = async (url, init = {}) => {
  const { pathname, search } = new URL(url);
  if(pathname === '/_dev/login') return { ok: true, status: 200, headers: new Headers(), json: async () => ({ token: 'tok' }) };
  asked.push([init.method || 'GET', pathname + search, init.body && JSON.parse(init.body)]);
  if(pathname === '/messages') return { ok: true, status: 200, headers: new Headers(), json: async () => messagesPart };
  if(pathname.endsWith('/files')) return { ok: true, status: 200, headers: new Headers(), json: async () => ({ files: [], sessions_done: 0, sessions_total: 0, cursor: null, data_version: 1 }) };
  if(typeof patchAnswer === 'number') return { ok: false, status: patchAnswer, headers: new Headers(), text: async () => '' };
  return { ok: true, status: 200, headers: new Headers(), json: async () => patchAnswer };
};

async function settle(){
  for(let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
}

beforeEach(async () => {
  resetState();
  state.conversations = [{ id: 'c0', name: 'Busy', total_messages: 2 }];
  state.blocks = [block(0, 0, '2026-03-02T10:00:00Z', '2026-03-02T10:10:00Z', counts({ messages: 1, automatic: { critical: 1, any: 1 }, both: { critical: 1, any: 1 } }))];
  state.dataVersion = 1;
  review.startReviewQuery();
  await settle();
  asked.length = 0;
});

test('a box saves all three flags with the handle; the counted session replaces the page\'s copy', async () => {
  patchAnswer = {
    message_id: 'm1', auto: {}, scanned: true, user: {}, data_version: 2,
    session: session('c0', 0, '2026-03-02T10:00:00Z', '2026-03-02T10:10:00Z', counts({ messages: 1, reviewed: 1, yours: { caps: 1, critical: 1, any: 1 }, both: { caps: 1, critical: 1, any: 1 } })),
  };
  state.selectedConversation = 0;
  edits.setRowOverrides('m1', 'caps', true);
  assert.deepEqual(state.overrides.m1, { caps: true, angry: false, critical: true }, 'shown at once');
  await settle();
  assert.deepEqual(asked[0], ['PATCH', '/conversations/c0/messages/m1/flags', { caps: true, angry: false, critical: true, handle: 'h1' }]);
  assert.equal(state.dataVersion, 2);
  assert.equal(state.blocks[0].counts.both.caps, 1);
  assert.match(page.el('calendarBody').innerHTML, /data-flag-type="caps"/);
  assert.match(page.el('convDetail').innerHTML, /1 ALL-CAPS/);
  assert.ok(asked.some(([, path]) => path.startsWith('/messages')), 'Review asks for its page again');
  assert.equal(page.el('saveStatus').textContent, 'Saved.');
});

test('Approve saves the flags as they are; an unknown row saves nothing; a refused save says so', async () => {
  patchAnswer = 500;
  edits.approveRow('m1');
  await settle();
  assert.deepEqual(asked[0][2], { caps: false, angry: false, critical: true, handle: 'h1' });
  assert.equal(page.el('saveStatus').textContent, 'Could not save to the server: server returned 500');
  asked.length = 0;
  edits.setRowOverrides('nope', 'caps', true);
  await settle();
  assert.deepEqual(asked, []);
});

test("a save whose session isn't on the page says so in the console", async () => {
  patchAnswer = { message_id: 'm1', data_version: 3, session: session('other', 4, '2026-03-02T10:00:00Z', '2026-03-02T10:10:00Z', counts()) };
  const warnings = [];
  const warn = console.warn;
  console.warn = (m) => warnings.push(m);
  edits.approveRow('m1');
  await settle();
  console.warn = warn;
  assert.match(warnings[0], /a saved flag's session \(other #4\) isn't on the page/);
  assert.equal(state.dataVersion, 3);
});

test('the show switches redraw every view and ask Review again', async () => {
  page.el('toggleShowAuto').checked = false;
  page.el('toggleShowUser').checked = true;
  edits.onVisibilityToggleChanged();
  await settle();
  assert.deepEqual([state.showAuto, state.showUser], [false, true]);
  assert.match(asked.at(-1)[1], /view=yours/);
  assert.doesNotMatch(page.el('calendarBody').innerHTML, /flag-icon/);
});
