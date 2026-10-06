// Review a page at a time (ui/views/review.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b, §8c): the
// first page shown as soon as its rows arrive, the count carried on with
// each part's cursor ("at least N" until it is done), one starting cursor
// kept per page of 50, a page asked for from its cursor, a changed data
// version starting over, and the filters each entry point sets. The page
// is a stand-in; `fetch` is a scripted server.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage, StubElement } from './page-stub.js';
import { state } from '../core/state.js';
import { resetState } from './fixtures.js';

const page = installPage();
globalThis.window = { location: { search: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
globalThis.requestAnimationFrame = (fn) => fn();
globalThis.CSS = { escape: (s) => s };
page.el('devLoginSub').value = 'alice';

const review = await import('../ui/views/review.js');

// The scripted server: a dev login, then each GET /messages answered by the
// next of `script` (a function of the query, or a status for a failure).
let script = [];
const asked = [];
globalThis.fetch = async (url) => {
  const path = url.replace('http://127.0.0.1:3000', '');
  if(path === '/_dev/login') return { ok: true, status: 200, headers: new Headers(), json: async () => ({ token: 'tok' }) };
  const query = Object.fromEntries(new URL(url).searchParams);
  asked.push(query);
  const next = script.shift();
  assert.ok(next !== undefined, `nothing scripted for ${path}`);
  if(typeof next === 'number') return { ok: false, status: next, headers: new Headers(), text: async () => '{"error":"broken"}' };
  const body = typeof next === 'function' ? next(query) : next;
  return { ok: true, status: 200, headers: new Headers(), json: async () => body };
};

async function settle(){
  for(let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
}

const row = (n) => ({
  kind: 'message', conversation_id: 'c0', message_id: `m${n}`, at: '2026-03-02T10:00:00Z', handle: `h${n}`,
  pieces: [{ type: 'text', text: `message ${n}` }], attachments: [],
  flags: { auto: null, user: { caps: null, critical: null, angry: null } }, reply: null,
});
const rows = (from, to) => Array.from({ length: to - from }, (_, i) => row(from + i));
const part = (extra) => ({ rows: [], matched: 0, notes: 0, page_starts: [], cursor: null, sessions_done: 1, sessions_total: 1, data_version: 5, ...extra });
const shownIds = () => [...page.el('reviewTable').innerHTML.matchAll(/data-msg-id="(m\d+)"/g)].map((m) => m[1]);

beforeEach(() => {
  resetState();
  state.conversations = [{ id: 'c0', name: 'Zero' }, { id: 'c1', name: 'One' }];
  page.el('view-review').classList.add('active');
  page.el('reviewFilter').value = 'all';
  page.el('reviewSearch').value = '';
  script = [];
  asked.length = 0;
});

test('before anything was asked, a save asks for nothing, and the replies switch waits for Review', async () => {
  // First in this file, so the module has never asked the server.
  page.el('view-review').classList.remove('active');
  review.refreshReviewPage();
  review.reloadReviewPage();
  await settle();
  assert.equal(asked.length, 0);
});

test('the first page shows as its rows arrive; the count carries on until the walk is done', async () => {
  script = [
    part({ rows: rows(0, 20), matched: 20, cursor: 'w1', sessions_done: 1, sessions_total: 3 }),
    part({ rows: rows(20, 50), matched: 60, page_starts: [{ page: 1, cursor: 'p1' }], cursor: 'w2', sessions_done: 2, sessions_total: 3 }),
    part({ matched: 120, notes: 2, page_starts: [{ page: 2, cursor: 'p2' }], cursor: null, sessions_done: 3, sessions_total: 3 }),
  ];
  review.startReviewQuery();
  assert.equal(page.el('reviewProgress').hidden, false);
  await settle();
  assert.deepEqual(asked.map((q) => [q.cursor, q.matched, q.notes, q.rows, q.until]), [
    [undefined, '0', '0', '50', 'end'], ['w1', '20', '0', '30', 'end'], ['w2', '60', '0', '0', 'end'],
  ]);
  assert.deepEqual([asked[0].flag, asked[0].view, asked[0].replies], ['all', 'both', 'false']);
  assert.equal(shownIds().length, 50);
  assert.equal(page.el('reviewCount').textContent, '118 messages');
  assert.match(page.el('pagination').innerHTML, /Page 1 of 3/);
  assert.equal(page.el('reviewProgress').hidden, true);
  assert.equal(review.reviewMessage('m3').handle, 'h3');

  // Next asks for page 2 from its cursor and stops once it has its rows.
  script = [part({ rows: rows(50, 80), matched: 80, cursor: 'x1' }), part({ rows: rows(80, 100), matched: 100, cursor: 'x2' })];
  page.el('nextPage').fireLast('click');
  await settle();
  assert.deepEqual(asked.slice(3).map((q) => [q.cursor, q.matched, q.rows, q.until]), [['p1', '50', '50', 'rows'], ['x1', '80', '20', 'rows']]);
  assert.deepEqual([shownIds()[0], shownIds().length], ['m50', 50]);
  assert.match(page.el('pagination').innerHTML, /Page 2 of 3/);
  // Back to the first page: no cursor.
  script = [part({ rows: rows(0, 50), matched: 50, cursor: 'y' })];
  page.el('prevPage').fireLast('click');
  await settle();
  assert.deepEqual([asked.at(-1).cursor, asked.at(-1).matched], [undefined, '0']);
});

test('until the walk is done the page says "at least"', async () => {
  let release;
  script = [
    part({ rows: rows(0, 50), matched: 70, page_starts: [{ page: 1, cursor: 'p1' }], cursor: 'w1' }),
    () => new Promise((r) => { release = r; }),
  ];
  review.startReviewQuery();
  await settle();
  assert.equal(page.el('reviewCount').textContent, 'at least 70 messages');
  assert.match(page.el('pagination').innerHTML, /Page 1 of at least 2/);
  assert.doesNotMatch(page.el('pagination').innerHTML, /id="nextPage" disabled/);
  assert.match(page.el('pagination').innerHTML, /id="prevPage" disabled/);
  // The bar's clock runs until the walk ends.
  assert.equal(page.el('reviewProgress').hidden, false);
  release(part({ matched: 70 }));
  await settle();
  assert.equal(page.el('reviewProgress').hidden, true);
});

test('a changed data version starts the walk over and says so; data that keeps changing stops', async () => {
  script = [part({ rows: rows(0, 50), matched: 60, cursor: 'w1', data_version: 5 }), part({ data_version: 6 }), part({ rows: rows(0, 3), matched: 3, data_version: 6 })];
  review.startReviewQuery();
  await settle();
  assert.equal(shownIds().length, 3);
  assert.equal(asked[2].cursor, undefined, 'started again from the beginning');
  script = [];
  for(let v = 1; v <= 5; v++) script.push(part({ matched: 60, cursor: 'w', data_version: v * 10 }), part({ data_version: v * 10 + 1 }));
  review.startReviewQuery();
  await settle();
  assert.match(page.el('reviewProgressLabel').textContent, /^Could not finish: your data kept changing/);
});

test('a page whose data changed starts everything over', async () => {
  script = [part({ rows: rows(0, 50), matched: 120, page_starts: [{ page: 1, cursor: 'p1' }], cursor: null })];
  review.startReviewQuery();
  await settle();
  script = [part({ rows: rows(50, 100), data_version: 9 }), part({ rows: rows(0, 2), matched: 2, data_version: 9 })];
  page.el('nextPage').fireLast('click');
  await settle();
  assert.deepEqual(shownIds(), ['m0', 'm1']);
  assert.match(page.el('pagination').innerHTML, /Page 1 of 1/);
});

test('a failed request is shown on the bar', async () => {
  script = [500];
  review.startReviewQuery();
  await settle();
  assert.equal(page.el('reviewProgress').hidden, false);
  assert.match(page.el('reviewProgressLabel').textContent, /^Could not finish: reading your messages failed \(500\): broken$/);
  script = [part({ rows: rows(0, 50), matched: 120, page_starts: [{ page: 1, cursor: 'p1' }] }), 500];
  review.startReviewQuery();
  await settle();
  page.el('nextPage').fireLast('click');
  await settle();
  assert.match(page.el('reviewProgressLabel').textContent, /failed \(500\)/);
});

test('after a save, the page on screen is asked for again from its cursor, and the count again', async () => {
  script = [part({ rows: rows(0, 50), matched: 120, page_starts: [{ page: 1, cursor: 'p1' }, { page: 2, cursor: 'p2' }] })];
  review.startReviewQuery();
  await settle();
  script = [part({ rows: rows(50, 100), cursor: 'z' })];
  page.el('nextPage').fireLast('click');
  await settle();
  asked.length = 0;
  script = [
    (q) => (q.until === 'end' ? part({ matched: 119, page_starts: [{ page: 1, cursor: 'p1' }, { page: 2, cursor: 'p2b' }] }) : part({ rows: rows(50, 100), cursor: 'z' })),
    (q) => (q.until === 'end' ? part({ matched: 119, page_starts: [{ page: 1, cursor: 'p1' }, { page: 2, cursor: 'p2b' }] }) : part({ rows: rows(50, 100), cursor: 'z' })),
  ];
  review.refreshReviewPage();
  await settle();
  assert.deepEqual(asked.map((q) => [q.until, q.cursor, q.rows]).sort(), [['end', undefined, '0'], ['rows', 'p1', '50']]);
  assert.equal(page.el('reviewCount').textContent, '119 messages');
  assert.match(page.el('pagination').innerHTML, /Page 2 of 3/);
  // On the first page the walk brings the rows itself.
  script = [part({ rows: rows(0, 50), matched: 120, page_starts: [{ page: 1, cursor: 'p1' }] })];
  page.el('prevPage').fireLast('click');
  await settle();
  asked.length = 0;
  script = [part({ rows: rows(0, 50), matched: 120 })];
  review.refreshReviewPage();
  await settle();
  assert.deepEqual(asked.map((q) => [q.until, q.rows]), [['end', '50']]);
});

test("Claude's replies switched on ask for the page on screen again, with them", async () => {
  script = [part({ rows: rows(0, 3), matched: 3 })];
  review.startReviewQuery();
  await settle();
  state.showReplies = true;
  script = [part({ rows: rows(0, 3) })];
  review.reloadReviewPage();
  await settle();
  assert.deepEqual([asked.at(-1).replies, asked.at(-1).until], ['true', 'rows']);
});

test('results for data or switches that changed are asked for when Review is next shown', async () => {
  page.el('view-review').classList.remove('active');
  review.markReviewStale();
  review.reloadReviewPage();
  await settle();
  assert.equal(asked.length, 0);
  page.el('view-review').classList.add('active');
  script = [part({ rows: rows(0, 1), matched: 1 })];
  review.showReviewTab();
  await settle();
  assert.equal(asked.length, 1);
  review.showReviewTab();
  await settle();
  assert.equal(asked.length, 1, 'shown again, nothing changed: nothing asked');
  script = [part({})];
  review.markReviewStale();
  await settle();
  assert.equal(asked.length, 2, 'stale while shown: asked at once');
});

test('a session, a file or an analysis point opens Review on its conversation and span, with its flag filter', async () => {
  script = [part({ rows: rows(0, 2), matched: 2 })];
  review.jumpToReview({ conv: 1, rangeStart: Date.parse('2026-03-02T10:00:00Z'), rangeEnd: Date.parse('2026-03-02T11:00:00Z'), flagType: 'angry', highlightIds: ['m1'], showReplies: true });
  const highlighted = new StubElement('tr');
  page.select('tr[data-msg-id="m1"], tr[data-reply-id="m1"]', [highlighted]);
  highlighted.scrollIntoView = () => { highlighted.scrolled = true; };
  await settle();
  assert.deepEqual(
    [asked[0].conversation, asked[0].from, asked[0].to, asked[0].span, asked[0].flag, asked[0].replies],
    ['c1', '2026-03-02T10:00:00.000Z', '2026-03-02T11:00:00.000Z', 'range', 'angry', 'true'],
  );
  assert.equal(page.el('toggleShowReplies').checked, true);
  assert.ok(highlighted.classes.has('row-highlight'));
  assert.equal(highlighted.scrolled, true);
  const banner = page.el('reviewFilterBanner');
  assert.match(banner.innerHTML, /Conversation: <strong>One<\/strong> · Time span:/);
  // "View entire conversation" drops the span; "View entire day" opens the day.
  script = [part({})];
  page.el('viewEntireConvBtn').fireLast('click');
  await settle();
  assert.deepEqual([asked.at(-1).conversation, asked.at(-1).from], ['c1', undefined]);
  script = [part({})];
  review.jumpToReview({ conv: 0, rangeStart: Date.parse('2026-03-02T10:00:00Z'), rangeEnd: Date.parse('2026-03-02T11:00:00Z') });
  await settle();
  script = [part({})];
  page.el('viewEntireDayBtn').fireLast('click');
  await settle();
  assert.deepEqual([asked.at(-1).span, asked.at(-1).from, asked.at(-1).conversation], ['day', '2026-03-02T00:00:00.000Z', undefined]);
});

test('a Calendar day, the days either side, and clearing the filter', async () => {
  script = [part({})];
  review.jumpToReviewDay('2026-03-02');
  await settle();
  assert.deepEqual([asked[0].span, asked[0].from, asked[0].to], ['day', '2026-03-02T00:00:00.000Z', '2026-03-02T23:59:59.999Z']);
  assert.match(page.el('reviewFilterBanner').innerHTML, /Day: <strong>/);
  script = [part({})];
  page.el('nextDayBtn').fireLast('click');
  await settle();
  assert.equal(asked.at(-1).from, '2026-03-03T00:00:00.000Z');
  script = [part({})];
  page.el('prevDayBtn').fireLast('click');
  await settle();
  assert.equal(asked.at(-1).from, '2026-03-02T00:00:00.000Z');
  script = [part({})];
  page.el('clearReviewFilter').fireLast('click');
  await settle();
  assert.deepEqual([asked.at(-1).span, asked.at(-1).conversation], [undefined, undefined]);
  assert.equal(page.el('reviewFilterBanner').hidden, true);
  script = [part({ rows: [row(1)], matched: 1 })];
  review.jumpToReview({});
  await settle();
  assert.equal(asked.at(-1).conversation, undefined);
  assert.match(page.el('reviewTable').innerHTML, /Zero/);
  state.conversations = [];
  review.renderReviewTable();
  assert.match(page.el('reviewTable').innerHTML, /\(a conversation no longer here\)/);
});

test('a search waits for typing to pause; the flag menu starts over at once', async () => {
  script = [part({})];
  page.el('reviewSearch').value = 'shout';
  review.searchChanged();
  review.searchChanged();
  await settle();
  assert.equal(asked.length, 0);
  await new Promise((r) => setTimeout(r, 350));
  await settle();
  assert.deepEqual(asked.map((q) => q.search), ['shout']);
  script = [part({})];
  review.showFirstReviewPage();
  await settle();
  assert.equal(asked.length, 2);
});

test('checkboxes, Approve and file cards call what main.js connected', async () => {
  const calls = [];
  review.setFlagEditHandlers((id, type, checked) => calls.push(['toggle', id, type, checked]), (id) => calls.push(['approve', id]));
  review.setFileOpener((file) => calls.push(['open', file]));
  const box = Object.assign(new StubElement('input'), { dataset: { id: 'm1', type: 'caps' }, checked: true });
  const approve = Object.assign(new StubElement('button'), { dataset: { id: 'm1' } });
  const card = Object.assign(new StubElement('button'), { dataset: { fileConv: 'c0', fileMsg: 'm1', fileNumber: '2' } });
  page.select('.flag-checkbox input:not([disabled])', [box]);
  page.select('.approve-btn', [approve]);
  page.select('#reviewTable .file-card:not([disabled])', [card]);
  review.renderReviewTable();
  box.fireLast('change');
  approve.fireLast('click');
  card.fireLast('click');
  assert.deepEqual(calls, [
    ['toggle', 'm1', 'caps', true], ['approve', 'm1'], ['open', { conversationId: 'c0', messageId: 'm1', number: 2 }],
  ]);
  state.showUser = false;
  review.renderReviewTable();
  assert.equal(box.listeners.change.length, 1, 'with your tags hidden, the boxes do nothing');
  page.select('.flag-checkbox input:not([disabled])', []);
  page.select('.approve-btn', []);
  page.select('#reviewTable .file-card:not([disabled])', []);
});

test('a page request replaced by a newer one fails quietly, with a note in the console', async () => {
  script = [part({ rows: rows(0, 50), matched: 150, page_starts: [{ page: 1, cursor: 'p1' }, { page: 2, cursor: 'p2' }] })];
  review.startReviewQuery();
  await settle();
  const notes = [];
  const info = console.info;
  console.info = (text) => notes.push(text);
  try{
    script = [500, part({ rows: rows(100, 150) })];
    page.el('nextPage').fireLast('click');
    page.el('nextPage').fireLast('click');
    await settle();
  } finally {
    console.info = info;
  }
  assert.deepEqual(notes, ['a replaced page request stopped: reading your messages failed (500): broken']);
  assert.equal(page.el('reviewProgress').hidden, true);
  assert.match(page.el('pagination').innerHTML, /Page 3 of 3/);
});

test("a session opened from elsewhere flashes its rows: all of yours, or those with the flag clicked, once a part holds one", async () => {
  const flagged = { ...row(2), flags: { auto: { caps: false, critical: true, angry: false }, user: { caps: null, critical: null, angry: null } } };
  const elements = {};
  for(const id of ['m0', 'm1', 'm2']){
    elements[id] = new StubElement('tr');
    page.select(`tr[data-msg-id="${id}"], tr[data-reply-id="${id}"]`, [elements[id]]);
  }
  // The first part holds nothing flagged; the second holds one.
  script = [part({ rows: rows(0, 2), matched: 2, cursor: 'x' }), part({ rows: [flagged], matched: 3 })];
  review.jumpToReview({ conv: 0, rangeStart: 0, rangeEnd: 1, highlightFlag: 'critical' });
  await settle();
  assert.deepEqual(['m0', 'm1', 'm2'].map((id) => elements[id].classes.has('row-highlight')), [false, false, true]);
  for(const id of ['m0', 'm1', 'm2']) elements[id].classes.delete('row-highlight');
  script = [part({ rows: rows(0, 2), matched: 2 })];
  review.jumpToReview({ conv: 0, rangeStart: 0, rangeEnd: 1, highlightFlag: 'all' });
  await settle();
  assert.deepEqual(['m0', 'm1'].map((id) => elements[id].classes.has('row-highlight')), [true, true]);
});
