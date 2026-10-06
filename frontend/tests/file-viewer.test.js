// The file viewer (ui/file-viewer.js, ui/render/file-view.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4): a card opens
// GET /files/…, the text is read from the address it gives, and the file is
// shown by its kind without running anything in it, with a download link.
// The page and highlight.js are stand-ins; `fetch` answers as the server.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installPage, StubElement } from './page-stub.js';

const page = installPage();
globalThis.window = { location: { search: '' } };
globalThis.localStorage = { getItem: () => null, setItem(){} };
page.el('devLoginSub').value = 'alice';
const made = [];
globalThis.URL.createObjectURL = (blob) => { made.push(blob); return `blob:${made.length}`; };
const revoked = [];
globalThis.URL.revokeObjectURL = (url) => revoked.push(url);

const { codeHtml, showFileIn } = await import('../ui/render/file-view.js');
const { connectFileViewer, openFileViewer } = await import('../ui/file-viewer.js');

test('code is coloured by highlight.js when it knows the language, and escaped otherwise', () => {
  assert.equal(codeHtml('<x>', 'python'), '&lt;x&gt;', 'without highlight.js');
  globalThis.hljs = {
    getLanguage: (l) => l === 'python',
    highlight: (text, { language }) => ({ value: `<span class="hljs-${language}">${text.length}</span>` }),
  };
  assert.equal(codeHtml('print(1)', 'python'), '<span class="hljs-python">8</span>');
  assert.equal(codeHtml('<x>', 'cobol'), '&lt;x&gt;');
  assert.equal(codeHtml('<x>', null), '&lt;x&gt;');
});

test('each kind is shown its own way, and only a web page has a source to switch to', () => {
  const body = new StubElement();
  const url = (blob) => `made:${blob.type}`;
  assert.equal(showFileIn(body, '<svg/>', { kind: 'svg' }, url), null);
  assert.deepEqual([body.children[0].tagName, body.children[0].src], ['IMG', 'made:image/svg+xml']);
  const page = showFileIn(body, '<script>x()</script>', { kind: 'web_page' }, url);
  const [frame, source] = body.children;
  assert.equal(frame.tagName, 'IFRAME');
  assert.equal(frame.getAttribute('sandbox'), '', 'every permission off');
  assert.equal(frame.srcdoc, '<script>x()</script>');
  assert.equal(source.hidden, true);
  page.source(true);
  assert.deepEqual([frame.hidden, source.hidden], [true, false]);
  assert.equal(showFileIn(body, '# Title', { kind: 'markdown' }, url), null);
  assert.equal(body.children[0].innerHTML, '<div class="md-heading h1">Title</div>');
  showFileIn(body, 'print(1)', { kind: 'code', language: 'python' }, url);
  assert.equal(body.children[0].children[0].innerHTML, '<span class="hljs-python">8</span>');
  showFileIn(body, 'plain <text>', { kind: 'text' }, url);
  assert.deepEqual([body.children[0].tagName, body.children[0].textContent], ['PRE', 'plain <text>']);
  assert.equal(body.children.length, 1, 'each file replaces the last');
});

function serve(...answers){
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(url.replace('http://127.0.0.1:3000', ''));
    const next = answers.shift();
    if(typeof next === 'number') return { ok: false, status: next, headers: new Headers(), text: async () => '{"error":"no such file"}' };
    const body = typeof next === 'string' ? next : JSON.stringify(next);
    return { ok: true, status: 200, headers: new Headers(), json: async () => JSON.parse(body), text: async () => body };
  };
  return asked;
}

test('a card opens the file it names, shown with its kind, its marks and a download link', async () => {
  connectFileViewer();
  const asked = serve({ token: 'tok' }, {
    url: '/_dev/local-storage/get/f', number: 2, name: 'report.html', kind: { kind: 'web_page' },
    contents: 'stored', may_have_changed_later: true,
  }, '<p>hi</p>');
  await openFileViewer({ conversationId: 'c', messageId: 'm', number: 2 });
  assert.deepEqual(asked, ['/_dev/login', '/files/c/m/2', '/_dev/local-storage/get/f']);
  assert.equal(page.el('fileViewer').hidden, false);
  assert.equal(page.el('fileViewerTitle').textContent, 'report.html');
  assert.equal(page.el('fileViewerNote').textContent, 'web page · this copy may have been changed later');
  const download = page.el('fileViewerDownload');
  assert.deepEqual([download.hidden, download.download], [false, 'report.html']);
  assert.equal(await made.at(-1).text(), '<p>hi</p>');
  const toggle = page.el('fileViewerSource');
  assert.equal(toggle.hidden, false);
  toggle.onclick();
  assert.equal(toggle.textContent, 'Show the page');
  toggle.onclick();
  assert.equal(toggle.textContent, 'Show source');
  // The backdrop closes it; a click inside the box doesn't.
  page.el('fileViewer').fire('click', new StubElement());
  assert.equal(page.el('fileViewer').hidden, false);
  page.el('fileViewer').fire('click');
  assert.equal(page.el('fileViewer').hidden, true);
  assert.ok(revoked.includes(download.href), 'its addresses are revoked on close');
});

test('a file the server cannot give says why; an answer for a closed viewer is dropped', async () => {
  serve(404);
  await openFileViewer({ conversationId: 'c', messageId: 'm', number: 0 });
  assert.match(page.el('fileViewerBody').children[0].textContent, /^Could not open the file: opening the file failed \(404\): no such file$/);
  serve({ url: '/f', name: 'a.txt', kind: { kind: 'text' }, contents: 'stored', may_have_changed_later: false }, 'text');
  const opening = openFileViewer({ conversationId: 'c', messageId: 'm', number: 0 });
  page.el('fileViewerClose').fire('click');
  await opening;
  assert.equal(page.el('fileViewer').hidden, true);
  assert.equal(page.el('fileViewerTitle').textContent, 'Opening the file…');
  serve(500);
  const failing = openFileViewer({ conversationId: 'c', messageId: 'm', number: 0 });
  page.el('fileViewerClose').fire('click');
  await failing;
  assert.equal(page.el('fileViewerBody').children.length, 0, 'a closed viewer shows no failure');
  serve({ url: '/f', name: 'a.txt', kind: { kind: 'text' }, contents: 'stored', may_have_changed_later: false }, 'text');
  await openFileViewer({ conversationId: 'c', messageId: 'm', number: 0 });
  assert.equal(page.el('fileViewerSource').hidden, true);
  assert.equal(page.el('fileViewerNote').textContent, 'text');
});
