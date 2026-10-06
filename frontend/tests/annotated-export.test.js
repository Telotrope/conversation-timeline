// The annotated download (ui/annotated-export.js): the server builds it in
// parts from the stored messages (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §8c), and the
// page joins every part's text, in order, into one file, saved when the
// last arrives. Node has no page, so the few elements the module touches
// are stand-ins, and `fetch` answers as the server would.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { PAGE_MESSAGES } from '../ui/widgets/page-messages.js';

globalThis.window = { location: { search: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };

function element(extra = {}){
  const classes = new Set();
  return {
    hidden: true, disabled: false, textContent: '', value: '', style: {},
    classList: { add: (c) => classes.add(c), remove: (...cs) => cs.forEach((c) => classes.delete(c)), has: (c) => classes.has(c) },
    ...extra,
  };
}
const els = {
  exportAnnotatedBtn: element(), exportProgress: element(), exportProgressFill: element(),
  exportProgressLabel: element(), saveStatus: element(), devLoginSub: element({ value: 'alice' }),
};
const saved = [];
globalThis.document = {
  getElementById: (id) => els[id] || null,
  createElement: (tag) => {
    assert.equal(tag, 'a');
    const a = { click(){ saved.push({ href: a.href, download: a.download }); } };
    return a;
  },
};
const blobs = new Map();
globalThis.URL.createObjectURL = (blob) => { const url = `blob:${blobs.size}`; blobs.set(url, blob); return url; };
globalThis.URL.revokeObjectURL = () => {};

const answer = (status, body) => ({
  ok: status >= 200 && status < 300, status, headers: new Headers(),
  json: async () => body, text: async () => JSON.stringify(body),
});

// The server: a dev login, then GET /export answered by `parts` in turn.
function serve(parts){
  const asked = [];
  globalThis.fetch = async (url) => {
    if(url.endsWith('/_dev/login')) return answer(200, { token: 'tok' });
    asked.push(url.replace('http://127.0.0.1:3000', ''));
    const next = parts.shift();
    return typeof next === 'number' ? answer(next, { error: 'boom' }) : answer(200, next);
  };
  return asked;
}

const { exportAnnotatedConversations, DOWNLOAD_NAME } = await import('../ui/annotated-export.js');

test('every part is asked for with the last cursor, and their text joined in order into one saved file', async () => {
  const asked = serve([
    { part: '{"conversations":[', flag_handles: {}, sessions_done: 1, sessions_total: 2, cursor: 'c1', data_version: 4 },
    { part: '{"uuid":"a"}]}', flag_handles: {}, sessions_done: 2, sessions_total: 2, cursor: null, data_version: 4 },
  ]);
  await exportAnnotatedConversations();
  assert.deepEqual(asked, ['/export', '/export?cursor=c1']);
  assert.equal(saved.length, 1);
  assert.equal(saved[0].download, DOWNLOAD_NAME);
  assert.deepEqual(JSON.parse(await blobs.get(saved[0].href).text()), { conversations: [{ uuid: 'a' }] });
  assert.equal(els.saveStatus.textContent, PAGE_MESSAGES['export.downloaded'].text({}));
  assert.equal(els.exportProgress.hidden, true);
  assert.equal(els.exportAnnotatedBtn.disabled, false);
});

test('a download whose data changes part-way starts again from the beginning', async () => {
  saved.length = 0;
  const asked = serve([
    { part: 'stale', sessions_done: 1, sessions_total: 2, cursor: 'c1', data_version: 4 },
    { part: 'x', sessions_done: 2, sessions_total: 2, cursor: null, data_version: 5 },
    { part: '[]', sessions_done: 2, sessions_total: 2, cursor: null, data_version: 5 },
  ]);
  await exportAnnotatedConversations();
  assert.deepEqual(asked, ['/export', '/export?cursor=c1', '/export']);
  assert.equal(await blobs.get(saved[0].href).text(), '[]');
});

test('a download the server refuses says so on the save line and on its bar, and saves nothing', async () => {
  saved.length = 0;
  serve([500]);
  await exportAnnotatedConversations();
  assert.equal(saved.length, 0);
  assert.match(els.saveStatus.textContent, /^Could not build the download: building your annotated download failed \(500\): boom$/);
  assert.equal(els.exportProgress.hidden, false);
  assert.ok(els.exportProgressFill.classList.has('is-error'));
  assert.equal(els.exportAnnotatedBtn.disabled, false);
});
