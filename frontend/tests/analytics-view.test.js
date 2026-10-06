// The Analytics tab (ui/views/analytics.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5c, §8c): three
// analyses computed in the page from the sessions' counts, two asked of the
// server, asked again while it answers "working", and stopped when
// Analytics is left. The page is a stand-in; `fetch` is a scripted server.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';
import { state } from '../core/state.js';
import { block, counts, resetState } from './fixtures.js';

const page = installPage();
globalThis.window = { location: { search: '', hash: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';

const { rerunAnalysis, runAnalysis } = await import('../ui/views/analytics.js');

let answers = [];
const asked = [];
globalThis.fetch = async (url) => {
  const { pathname, search } = new URL(url);
  if(pathname === '/_dev/login') return { ok: true, status: 200, headers: new Headers(), json: async () => ({ token: 'tok' }) };
  asked.push(pathname + search);
  const next = answers.shift();
  if(typeof next === 'number') return { ok: false, status: next, headers: new Headers(), text: async () => '{"error":"broken"}' };
  const body = typeof next === 'function' ? await next() : next;
  return { ok: true, status: 200, headers: new Headers(), json: async () => body };
};

async function settle(){
  for(let i = 0; i < 30; i++) await new Promise((r) => setTimeout(r, 0));
}

beforeEach(() => {
  resetState();
  state.conversations = [{ name: 'Busy' }, { name: 'Quiet' }];
  state.blocks = [
    block(0, 0, '2026-03-02T10:00:00Z', '2026-03-02T10:20:00Z', counts({ messages: 2, both: { angry: 1, any: 1 } })),
    block(0, 1, '2026-03-03T09:00:00Z', '2026-03-03T09:10:00Z', counts({ messages: 2 })),
    block(1, 0, '2026-03-04T09:00:00Z', '2026-03-04T09:10:00Z', counts({ messages: 1 })),
  ];
  state.dataVersion = 1;
  page.el('view-analytics').classList.add('active');
  answers = [];
  asked.length = 0;
});

const main = () => page.el('analyticsMain').innerHTML;

test('friction, session length and idle time are computed in the page and drawn', async () => {
  runAnalysis('friction', {});
  await settle();
  assert.match(main(), /<h3>Friction ranking<\/h3>/);
  assert.match(main(), /data-conv="0"[\s\S]*<td>Busy<\/td>\s*<td>4<\/td>\s*<td>1<\/td>\s*<td class="pct">25.0%<\/td>/);
  page.el('frictionBySession').fireLast('click');
  await settle();
  assert.match(main(), /Busy — Monday/);
  runAnalysis('length', {});
  await settle();
  assert.match(main(), /<span class="label">sessions<\/span>/);
  runAnalysis('idlegap', {});
  await settle();
  assert.match(main(), /2 sessions excluded as a conversation's first session/);
  assert.deepEqual(asked, [], 'nothing asked of the server');
});

test('an analysis left before it finishes stops, and carries on when shown again', async () => {
  page.el('view-analytics').classList.remove('active');
  runAnalysis('length', {});
  await settle();
  assert.doesNotMatch(main(), /Session length/);
  page.el('view-analytics').classList.add('active');
  rerunAnalysis();
  await settle();
  assert.match(main(), /Session length vs. flag rate/);
});

test('the server analyses are asked again while working, with the view, time zone and options', async () => {
  answers = [
    { status: 'working', sessions_done: 1, sessions_total: 3, data_version: 1 },
    { status: 'done', data_version: 1, numbers: { kind: 'trend', granularity: 'month', buckets: {
      '2026-03': { total: 4, flagged: 1 }, '2026-02': { total: 2, flagged: 2 }, '2026-01': { total: 0, flagged: 0 },
    } } },
  ];
  runAnalysis('trend', { granularity: 'month' });
  await settle();
  const tz = encodeURIComponent(Intl.DateTimeFormat().resolvedOptions().timeZone);
  assert.deepEqual(asked, [`/analyses/trend?view=both&tz=${tz}&granularity=month`, `/analyses/trend?view=both&tz=${tz}&granularity=month`]);
  assert.match(main(), /<h3>Flag rate over time<\/h3>/);
  const chart = page.el('trendChart').innerHTML;
  assert.match(chart, /2026-02: 100.0% \(2\/2\)[\s\S]*2026-03: 25.0% \(1\/4\)/);
  assert.doesNotMatch(chart, /2026-01/, 'a month with nothing counted has no rate');
  answers = [{ status: 'done', data_version: 1, numbers: { kind: 'time_of_day',
    by_hour: Array.from({ length: 24 }, (_, h) => ({ total: h === 9 ? 2 : 0, flagged: h === 9 ? 1 : 0 })),
    by_dow: Array.from({ length: 7 }, () => ({ total: 0, flagged: 0 })) } }];
  state.showUser = false;
  runAnalysis('timeofday', {});
  await settle();
  assert.match(asked.at(-1), /^\/analyses\/time-of-day\?view=automatic&tz=/);
  assert.match(page.el('hourChart').innerHTML, /9:00 — 50.0% \(1\/2\)/);
  answers = [{ status: 'done', data_version: 1, numbers: { kind: 'trend', granularity: 'week', buckets: {} } }];
  runAnalysis('trend', {});
  await settle();
  assert.match(asked.at(-1), /granularity=week/);
});

test('leaving Analytics stops asking; a failure is shown on the bar', async () => {
  let answer;
  answers = [() => new Promise((r) => { answer = r; })];
  runAnalysis('trend', {});
  await settle();
  page.el('view-analytics').classList.remove('active');
  answer({ status: 'working', sessions_done: 1, sessions_total: 3, data_version: 1 });
  await settle();
  assert.equal(asked.length, 1);
  page.el('view-analytics').classList.add('active');
  answers = [500];
  runAnalysis('timeofday', {});
  await settle();
  assert.match(page.el('analyticsProgressLabel').textContent, /^Could not finish: computing the analysis failed \(500\): broken$/);
});
