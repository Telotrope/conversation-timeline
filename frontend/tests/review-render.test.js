// Drawing Review's rows (ui/render/reply-markup.js and
// ui/render/review-table.js; plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4, §4c, §4d):
// cited spans with numbered links placed before the Markdown is formatted,
// file cards where the message presented them, notes in their own rows, and
// everything from a message escaped.

import { test, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { installPage } from './page-stub.js';
import { state } from '../core/state.js';
import { messageOf, overridesOf } from '../core/review-rows.js';
import { contentHtml, fileCardHtml, textPieceHtml } from '../ui/render/reply-markup.js';
import { reviewTableHtml } from '../ui/render/review-table.js';
import { resetState } from './fixtures.js';

installPage();
beforeEach(resetState);

const web = (address) => ({ kind: 'web', address });

test('a cited span gets a numbered link after it, made before the Markdown is formatted', () => {
  const html = textPieceHtml({ type: 'text', text: '**Paris** is big.', citations: [{ start: 0, end: 9, address: web('https://paris.example/?a="1"') }] });
  assert.equal(html, '<p class="md-p"><strong>Paris</strong><sup class="cite"><a href="https://paris.example/?a=&quot;1&quot;" target="_blank" rel="noopener noreferrer" title="https://paris.example/?a=&quot;1&quot;">1</a></sup> is big.</p>');
});

test('an address that is not a web address is shown, never linked', () => {
  const html = textPieceHtml({ type: 'text', text: 'x', citations: [{ start: 0, end: 1, address: { kind: 'other', address: 'javascript:alert(1)' } }] });
  assert.equal(html, '<p class="md-p">x<sup class="cite" title="javascript:alert(1)">1</sup></p>');
  assert.equal(textPieceHtml({ type: 'text', text: '<b>hi</b>' }), '<p class="md-p">&lt;b&gt;hi&lt;/b&gt;</p>');
});

const stored = { number: 0, name: 'chart <1>.svg', kind: { kind: 'svg' }, contents: 'stored', may_have_changed_later: false };
const missing = { number: 1, name: 'deck.pptx', kind: { kind: 'other' }, contents: 'not_in_export', may_have_changed_later: false };

test("a file's card opens it when its contents are stored, and says so when they aren't", () => {
  assert.equal(fileCardHtml(stored, 'c"1', 'm1'),
    '<button type="button" class="file-card" data-file-conv="c&quot;1" data-file-msg="m1" data-file-number="0">'
    + '<span class="file-card-name">chart &lt;1&gt;.svg</span><span class="file-card-kind">drawing</span></button>');
  const card = fileCardHtml(missing, 'c', 'm');
  assert.match(card, /class="file-card is-missing" disabled/);
  assert.match(card, /<span class="file-card-note">not included in the export<\/span>/);
  assert.match(fileCardHtml({ ...stored, may_have_changed_later: true }, 'c', 'm'), /this copy may have been changed later/);
});

test('cards sit where the message presented them; attachments follow the text', () => {
  const html = contentHtml([{ type: 'text', text: 'Before.' }, { type: 'file', file: stored }, { type: 'text', text: 'After.' }],
    [missing], 'c', 'm');
  const order = ['Before.', 'chart &lt;1&gt;.svg', 'After.', 'file-cards', 'deck.pptx'].map((s) => html.indexOf(s));
  assert.deepEqual([...order].sort((a, b) => a - b), order);
  assert.equal(contentHtml([], [], 'c', 'm'), '');
});

const conversations = [{ id: 'c', name: 'Plans & <ideas>' }, { id: 'kept', name: 'Kept' }];
const helpers = {
  conversationName: (id) => conversations.find((c) => c.id === id).name,
  conversationIndex: (id) => conversations.findIndex((c) => c.id === id),
};

function messageRow(extra = {}){
  return {
    kind: 'message', conversation_id: 'c', message_id: 'm1', at: '2026-03-02T10:00:00Z', handle: 'h',
    pieces: [{ type: 'text', text: 'hello' }], attachments: [],
    flags: { auto: { caps: true, critical: false, angry: false }, user: { caps: null, critical: null, angry: null } },
    reply: { message_id: 'r1', pieces: [{ type: 'text', text: 'a reply' }] },
    ...extra,
  };
}

function table(rows){
  const messages = new Map();
  for(const row of rows.filter((r) => r.kind === 'message')){
    messages.set(row.message_id, messageOf(row));
    const stated = overridesOf(row);
    if(stated) state.overrides[row.message_id] = stated;
  }
  return reviewTableHtml(rows, messages, helpers);
}

test('a message row: when, its conversation escaped, its text, a box per flag and Approve', () => {
  const html = table([messageRow()]);
  assert.match(html, /<tr data-msg-id="m1">/);
  assert.match(html, /Plans &amp; &lt;ideas&gt;/);
  assert.match(html, /<td class="msg-text"><p class="md-p">hello<\/p><\/td>/);
  assert.match(html, /data-type="caps" checked/);
  assert.match(html, /title="automatic tag from keyword\/sentiment heuristic"/);
  assert.match(html, /<button class="approve-btn" data-id="m1">Approve<\/button><span class="review-status">Not reviewed<\/span>/);
  assert.doesNotMatch(html, /claude-reply-row/, 'replies only with the switch on');
});

test("Claude's reply follows its message with the switch on; a message of unknown time says so", () => {
  state.showReplies = true;
  const html = table([messageRow({ at: null, pieces: [] }), messageRow({ message_id: 'm2', reply: null }), messageRow({ message_id: 'm3', reply: { message_id: 'r3', pieces: [] } })]);
  assert.match(html, /<td class="when">time unknown<\/td>/);
  assert.match(html, /<em class="no-text">\(no text\)<\/em>/);
  assert.match(html, /<tr class="claude-reply-row" data-reply-id="r1">\s*<td colspan="7"><span class="who-label">Claude<\/span><p class="md-p">a reply<\/p>/);
  assert.equal((html.match(/claude-reply-row/g) || []).length, 1, 'a missing or empty reply adds no row');
});

test('each pair of switches draws its own boxes', () => {
  const reviewed = messageRow({ flags: { auto: null, user: { caps: false, critical: false, angry: true } } });
  state.showAuto = false;
  let html = table([reviewed]);
  assert.match(html, /<input type="checkbox" data-id="m1" data-type="angry" checked>/);
  assert.match(html, /<span class="review-status is-reviewed">Reviewed<\/span>/);
  state.showAuto = true;
  state.showUser = false;
  html = table([messageRow()]);
  assert.match(html, /<input type="checkbox" disabled checked>/);
  assert.doesNotMatch(html, /approve-btn|review-status/);
  state.showAuto = false;
  html = table([messageRow({ flags: { auto: null, user: { caps: null, critical: null, angry: null } } })]);
  assert.doesNotMatch(html, /flag-checkbox|<th>All caps/);
  state.showReplies = true;
  assert.match(table([messageRow()]), /colspan="3"/);
});

test('a note sits in its own row, linking to the conversation a branch was kept as', () => {
  const note = (kept_as) => ({
    kind: 'note', key: { conversation_id: 'c', at: '2026-03-02T10:00:00Z', id: 'n' }, last_at: '2026-03-02T10:01:00Z',
    messages: 2, words_not_repeated: 150, replaced_by: 'm1', kept_as,
  });
  let html = table([note('kept')]);
  assert.match(html, /<tr class="note-row"><td colspan="7">An earlier branch of this conversation was kept as <a href="#conversations\/1">its own conversation<\/a>: 2 messages/);
  html = table([note('gone')]);
  assert.match(html, /was kept as its own conversation: 2 messages/);
  html = table([note(null)]);
  assert.match(html, /was pruned here: 2 messages \(150 words not repeated below\)/);
});
