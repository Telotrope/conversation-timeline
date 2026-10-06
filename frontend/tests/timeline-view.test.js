// Opening the timeline and drawing it from sessions (ui/timeline-load.js,
// ui/views/calendar.js, ui/views/conversations.js, ui/views/header.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §6, §8b):
// records then sessions read in parts at one data version, drawn in turns,
// the same flags on every day a session touches, and every click opening
// Review on the right span with the right filter. The page is a stand-in;
// `fetch` is a scripted server. TZ=UTC, so local days are UTC days.

import { test, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';
import { installPage, StubElement } from './page-stub.js';
import { state } from '../core/state.js';
import { counts, resetState, session } from './fixtures.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
globalThis.requestAnimationFrame = (fn) => fn();
globalThis.CSS = { escape: (s) => s };
page.el('devLoginSub').value = 'alice';
page.el('reviewFilter').value = 'all';

const { loadTimeline, readTimeline } = await import('../ui/timeline-load.js');
const { connectCalendar, renderCalendar } = await import('../ui/views/calendar.js');
const { connectConversations, renderConvList, selectConversation } = await import('../ui/views/conversations.js');
const { renderSubtitle } = await import('../ui/views/header.js');
const { hideLoadProgress } = await import('../ui/widgets/status-indicators.js');

// The load bar's clock runs until the page hides the bar; nothing here does.
after(hideLoadProgress);

// The server: each route answered by the next of its list; /messages
// always answers with no rows.
let routes = {};
const asked = [];
globalThis.fetch = async (url) => {
  const { pathname, search } = new URL(url);
  asked.push(pathname + search);
  let body;
  if(pathname === '/_dev/login') body = { token: 'tok' };
  else if(pathname === '/messages') body = { rows: [], matched: 0, notes: 0, page_starts: [], cursor: null, sessions_done: 0, sessions_total: 0, data_version: 1 };
  else{
    const list = routes[pathname];
    assert.ok(list && list.length, `nothing scripted for ${pathname}`);
    body = list.shift();
  }
  if(typeof body === 'number') return { ok: false, status: body, headers: new Headers(), text: async () => '{"error":"broken"}' };
  return { ok: true, status: 200, headers: new Headers(), json: async () => body };
};

async function settle(){
  for(let i = 0; i < 20; i++) await new Promise((r) => setImmediate(r));
}

const record = (id, name, extra = {}) => ({
  conversation_id: id, name, message_count: 4, untimed: 0, span: { start: '2026-03-01T00:00:00Z', end: '2026-03-02T00:00:00Z' },
  source: { file_name: 'conversations.json' }, participants: null, medium: null, details_origin: 'guessed',
  branch_of: null, branches: [], ...extra,
});
const flagged = counts({ messages: 3, automatic: { angry: 1, any: 1 }, both: { angry: 1, caps: 1, any: 2 }, yours: { caps: 1, any: 1 } });
const sessions = [
  session('c0', 0, '2026-03-01T23:50:00Z', '2026-03-02T00:20:00Z', flagged),
  session('c0', 1, '2026-03-05T10:00:00Z', '2026-03-05T10:30:00Z', counts({ messages: 1 })),
  session('c1', 0, '2026-03-02T09:00:00Z', '2026-03-02T09:05:00Z', counts({ messages: 2 })),
  session('gone', 0, '2026-03-02T09:00:00Z', '2026-03-02T09:05:00Z', counts({ messages: 2 })),
];

function scriptTimeline(version = 1){
  routes['/conversations'] = [
    { conversations: [record('c0', 'Plans', { branches: ['c1'] })], total: 2, cursor: 'k', data_version: version },
    { conversations: [record('c1', '', { branch_of: 'c0', untimed: 2 })], total: 2, cursor: null, data_version: version },
  ];
  routes['/sessions'] = [
    { sessions: sessions.slice(0, 2), total: 4, cursor: 's', data_version: version },
    { sessions: sessions.slice(2), total: 4, cursor: null, data_version: version },
  ];
  routes['/uploads'] = [{ uploads: [], total: 0, cursor: null, data_version: version }];
}

beforeEach(() => {
  resetState();
  routes = {};
  asked.length = 0;
});

test('the timeline is read in parts, then drawn: sessions, the Calendar, the list, the title line', async () => {
  scriptTimeline();
  const warnings = [];
  const warn = console.warn;
  console.warn = (m) => warnings.push(m);
  assert.equal(await loadTimeline('tok'), true);
  console.warn = warn;
  assert.deepEqual(asked, ['/conversations', '/conversations?cursor=k', '/sessions', '/sessions?cursor=s', '/uploads']);
  assert.deepEqual(state.conversations.map((c) => [c.name, c.id, c.untimed]), [['Plans', 'c0', 0], ['(untitled)', 'c1', 2]]);
  assert.equal(state.dataVersion, 1);
  assert.equal(state.blocks.length, 3);
  assert.match(warnings[0], /conversation gone, which has no record, is not drawn/);
  const calendar = page.el('calendarBody').innerHTML;
  // The session crossing midnight is drawn on both days, each piece with its flags.
  assert.equal((calendar.match(/data-block-idx="0"/g) || []).length, 2);
  assert.equal((calendar.match(/data-flag-type="angry" title="1 angry/g) || []).length, 2);
  assert.equal((calendar.match(/data-flag-type="caps" title="1 ALL-CAPS/g) || []).length, 2);
  assert.match(calendar, /data-day="2026-03-01"[\s\S]*data-day="2026-03-02"[\s\S]*data-day="2026-03-05"/);
  assert.match(calendar, /<div class="month-heading">March 2026<\/div>/);
  assert.match(page.el('convItems').innerHTML, /<span class="flag-icon angry">!<\/span> <span class="flag-icon caps">A<\/span> Plans/);
  assert.equal(page.el('subtitle').textContent, '2 conversations, 8 messages, 2026-03-01 to 2026-03-05.');
  // Drawn within one turn, so the bar's last words are the sessions received.
  assert.equal(page.el('loadProgressLabel').textContent, 'Receiving your sessions — 4 of 4');
});

test('records and sessions read at different data versions are read again; data that keeps changing gives up', async () => {
  scriptTimeline(1);
  routes['/sessions'] = [{ sessions: [], total: 0, cursor: null, data_version: 2 }];
  routes['/conversations'].push(
    { conversations: [record('c0', 'Plans')], total: 1, cursor: null, data_version: 2 },
  );
  routes['/sessions'].push({ sessions: [], total: 0, cursor: null, data_version: 2 });
  const read = await readTimeline('tok');
  assert.equal(read.dataVersion, 2);
  assert.equal(page.el('loadProgressLabel').textContent.startsWith('Receiving your sessions'), true);
  let v = 0;
  routes['/conversations'] = new Proxy([], { get: (t, k) => (k === 'length' ? 1 : k === 'shift' ? () => ({ conversations: [], total: 0, cursor: null, data_version: v++ }) : t[k]) });
  routes['/sessions'] = new Proxy([], { get: (t, k) => (k === 'length' ? 1 : k === 'shift' ? () => ({ sessions: [], total: 0, cursor: null, data_version: 1000 + v }) : t[k]) });
  await assert.rejects(readTimeline('tok'), /your data kept changing while the timeline was read/);
});

test('a user with no conversations has no timeline to draw', async () => {
  routes['/conversations'] = [{ conversations: [], total: 0, cursor: null, data_version: 1 }];
  routes['/sessions'] = [{ sessions: [], total: 0, cursor: null, data_version: 1 }];
  assert.equal(await loadTimeline('tok'), false);
  renderSubtitle();
  assert.equal(page.el('subtitle').textContent, '0 conversations, 0 messages.');
});

test('the switches change which flags the Calendar and the list show', async () => {
  scriptTimeline();
  await loadTimeline('tok');
  state.showAuto = false;
  renderCalendar();
  renderConvList('');
  assert.doesNotMatch(page.el('calendarBody').innerHTML, /data-flag-type="angry"/);
  assert.match(page.el('calendarBody').innerHTML, /data-flag-type="caps"/);
  state.showUser = false;
  renderCalendar();
  assert.doesNotMatch(page.el('calendarBody').innerHTML, /flag-icon/);
  renderConvList('PLA');
  assert.match(page.el('convItems').innerHTML, /data-idx="0"/);
  assert.doesNotMatch(page.el('convItems').innerHTML, /data-idx="1"/);
});

// A click on `target`, whose closest(selector) finds what `found` maps.
function clickOn(found){
  const target = new StubElement('span');
  target.closest = (selector) => found[selector] || null;
  return target;
}
const withData = (data) => Object.assign(new StubElement('div'), { dataset: data });

test('Calendar clicks open Review: a flag icon with its filter, a bar with all, a day as the day', async () => {
  scriptTimeline();
  await loadTimeline('tok');
  connectCalendar();
  const body = page.el('calendarBody');
  const bar = withData({ blockIdx: '0' });
  asked.length = 0;
  body.fire('click', clickOn({ '.bar-flags .flag-icon': withData({ flagType: 'angry' }), '.bar': bar }));
  await settle();
  assert.match(asked.at(-1), /conversation=c0&flag=angry&view=both&replies=false&from=2026-03-01T23%3A50%3A00.000Z&to=2026-03-02T00%3A20%3A00.000Z&span=range/);
  body.fire('click', clickOn({ '.bar': bar }));
  await settle();
  assert.match(asked.at(-1), /flag=all/);
  body.fire('click', clickOn({ '.day-label[data-day]': withData({ day: '2026-03-05' }) }));
  await settle();
  assert.match(asked.at(-1), /span=day/);
  const before = asked.length;
  body.fire('click', clickOn({}));
  await settle();
  assert.equal(asked.length, before, 'a click elsewhere does nothing');
});

test("an open conversation shows its details, branches, sessions, flags and files, each linking into Review", async () => {
  scriptTimeline();
  await loadTimeline('tok');
  connectConversations();
  const edits = [];
  const { setConversationEditHandler } = await import('../ui/views/conversations.js');
  setConversationEditHandler((id) => edits.push(id));
  routes['/conversations/c0/files'] = [
    { files: [{ message_id: 'r1', at: '2026-03-02T00:10:00Z', sender: 'assistant', number: 0, name: 'plan <1>.md', kind: { kind: 'markdown' }, contents: 'stored', may_have_changed_later: false }],
      sessions_done: 1, sessions_total: 2, cursor: 'f', data_version: 1 },
    { files: [{ message_id: 'm9', at: null, sender: 'human', number: 0, name: 'scan.pdf', kind: { kind: 'other' }, contents: 'not_in_export', may_have_changed_later: false }],
      sessions_done: 2, sessions_total: 2, cursor: null, data_version: 1 },
  ];
  page.el('convItems').fire('click', clickOn({ '.conv-item': withData({ idx: '0' }) }));
  await settle();
  const detail = page.el('convDetail').innerHTML;
  assert.match(detail, /<h3>Plans<\/h3>/);
  assert.match(detail, /Earlier branches kept as their own conversations: <a href="#conversations\/1">\(untitled\)<\/a>\./);
  assert.match(detail, /1 angry/);
  assert.match(detail, /1 ALL-CAPS/);
  const files = page.el('convFiles').innerHTML;
  assert.match(files, /data-message-id="r1" data-at="2026-03-02T00:10:00Z" data-sender="assistant">plan &lt;1&gt;.md<\/a>/);
  assert.match(files, /Markdown · from Claude/);
  assert.match(files, /file · from you, time unknown · not included in the export/);
  page.el('editConversationBtn').fireLast('click');
  assert.deepEqual(edits, ['c0']);

  // A file opens Review on the session holding it, with Claude's replies.
  asked.length = 0;
  const detailEl = page.el('convDetail');
  detailEl.fire('click', clickOn({ '.file-index-link': withData({ messageId: 'r1', at: '2026-03-02T00:10:00Z', sender: 'assistant' }) }));
  await settle();
  assert.match(asked.at(-1), /conversation=c0&flag=all&view=both&replies=true&from=2026-03-01T23%3A50/);
  detailEl.fire('click', clickOn({ '.file-index-link': withData({ messageId: 'm9', at: '', sender: 'human' }) }));
  await settle();
  assert.doesNotMatch(asked.at(-1), /from=/);
  // A session's flag icon, and its row.
  detailEl.fire('click', clickOn({ '.flag-icon[data-flag-type]': withData({ blockIdx: '0', flagType: 'caps' }), '.session-row': withData({ blockIdx: '0' }) }));
  await settle();
  assert.match(asked.at(-1), /flag=caps/);
  detailEl.fire('click', clickOn({ '.session-row': withData({ blockIdx: '1' }) }));
  await settle();
  assert.match(asked.at(-1), /flag=all&view=both&replies=true&from=2026-03-05/);
  const before = asked.length;
  detailEl.fire('click', clickOn({}));
  page.el('convItems').fire('click', clickOn({}));
  assert.equal(asked.length, before);
  // "Chat message review" opens the whole conversation.
  page.el('chatReviewLink').fireLast('click');
  await settle();
  assert.doesNotMatch(asked.at(-1), /from=/);

  // Drawn again (a flag saved), its files come from what was shown, not the server.
  asked.length = 0;
  selectConversation(0);
  await settle();
  assert.deepEqual(asked, []);
  assert.equal(page.el('convFiles').innerHTML, files);
});

test("a conversation kept from a branch links back; one without files says so; a failed list says why", async () => {
  scriptTimeline();
  await loadTimeline('tok');
  state.records.get('c1').branch_of = 'nowhere';
  routes['/conversations/c1/files'] = [{ files: [], sessions_done: 0, sessions_total: 0, cursor: null, data_version: 1 }];
  selectConversation(1);
  await settle();
  assert.match(page.el('convDetail').innerHTML, /An earlier branch of a conversation no longer here, kept as its own conversation\./);
  assert.match(page.el('convFiles').innerHTML, /No files were presented or attached in this conversation\./);
  routes['/conversations/c0/files'] = [500];
  selectConversation(0);
  await settle();
  assert.match(page.el('convFilesLabel').textContent, /^Could not finish: reading the conversation's files failed \(500\): broken$/);
  // No record, no details line.
  state.records.delete('c0');
  routes['/conversations/c0/files'] = [{ files: [], sessions_done: 0, sessions_total: 0, cursor: null, data_version: 1 }];
  selectConversation(0);
  assert.doesNotMatch(page.el('convDetail').innerHTML, /conv-details/);
  // An answer for a conversation no longer open is dropped: c1's list,
  // shown before, is the one on screen.
  selectConversation(1);
  await settle();
  assert.match(page.el('convFiles').innerHTML, /No files/);
  routes['/conversations/c0/files'] = [{ files: [], sessions_done: 0, sessions_total: 0, cursor: 'x', data_version: 1 }, 500];
  const { forgetConversationFiles } = await import('../ui/views/conversations.js');
  forgetConversationFiles();
  selectConversation(0);
  selectConversation(1);
  routes['/conversations/c1/files'] = [{ files: [], sessions_done: 0, sessions_total: 0, cursor: null, data_version: 1 }];
  await settle();
  assert.doesNotMatch(page.el('convFilesLabel').textContent, /Could not finish/, "a failure for a conversation no longer open isn't shown");
});
