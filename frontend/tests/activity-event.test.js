import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  TEXT_CAP, S3_EXPORT_ROUTE, S3_UPLOAD_ROUTE, SHOWN_PLACES,
  changeEvent, cleanText, clickEvent, describeElement, isTextBox, requestEvent, routeTemplate,
  shownEvent, submitEvent, viewEvent,
} from '../core/activity-event.js';
import { el } from './fake-dom.js';

const MESSAGE_TEXT = 'my secret message about the merger';

// The review table as the page draws it (ui/views/review.js): one row per
// message, its text in a cell, three flag checkboxes and an Approve button.
function reviewRow(){
  const caps = el('input', { type: 'checkbox', checked: true, attrs: { 'data-id': '3|2024-01-01T10:00:00Z', 'data-type': 'caps' } });
  const approve = el('button', { className: 'approve-btn', text: 'Approve', attrs: { 'data-id': '3|2024-01-01T10:00:00Z' } });
  const textSpan = el('span', { text: MESSAGE_TEXT });
  const textCell = el('td', { className: 'msg-text' }, textSpan);
  const row = el('tr', { attrs: { 'data-msg-id': '3|2024-01-01T10:00:00Z' } },
    el('td', { className: 'conv-name', text: 'Project Falcon' }),
    textCell,
    el('td', { className: 'flag-cell' }, el('div', { className: 'flag-checkbox' }, caps)),
    el('td', { className: 'flag-cell' }, approve));
  el('div', { id: 'reviewTable' }, el('table', {}, row));
  return { caps, approve, textSpan, row };
}

test('an element is described by its id, never its wording', () => {
  const button = el('button', { id: 'loadBtn', text: '  Load\n ' });
  assert.deepEqual(describeElement(button), { tag: 'button', id: 'loadBtn' });
});

test('a click on a span inside a tab button describes the button by its tab', () => {
  const inner = el('span', { text: 'Review' });
  el('nav', {}, el('button', { attrs: { 'data-tab': 'review' } }, inner));
  assert.deepEqual(clickEvent(inner), { kind: 'click', target: { tag: 'button', tab: 'review' } });
});

test("no label's text, aria-label or button text; name and data-analysis are kept", () => {
  const box = el('input', { id: 'autoDetectCheckbox', type: 'checkbox', checked: false });
  el('label', { text: 'Scan for flags ' }, box);
  assert.deepEqual(describeElement(box), { tag: 'input', id: 'autoDetectCheckbox' });
  const named = el('button', { text: 'x', attrs: { 'aria-label': 'Close' } });
  assert.deepEqual(describeElement(named), { tag: 'button' });
  const radio = el('input', { type: 'radio', attrs: { name: 'granularity' } });
  assert.deepEqual(describeElement(radio), { tag: 'input', name: 'granularity' });
  const analysis = el('button', { className: 'analytics-item', text: 'Friction', attrs: { 'data-analysis': 'friction' } });
  assert.deepEqual(describeElement(analysis), { tag: 'button', analysis: 'friction' });
  assert.doesNotMatch(JSON.stringify([box, named, analysis].map(describeElement)), /Scan|Close|Friction/);
});

test('plain elements get no label: they could hold any text', () => {
  assert.deepEqual(describeElement(el('p', { id: 'subtitle', text: '812 conversations' })), { tag: 'p', id: 'subtitle' });
  assert.deepEqual(describeElement(el('div', { text: 'anything' })), { tag: 'div' });
});

test('a review-row checkbox gives its message id and column, and no cell text', () => {
  const { caps } = reviewRow();
  const event = changeEvent(caps);
  assert.deepEqual(event, {
    kind: 'change',
    target: { tag: 'input', message_id: '3|2024-01-01T10:00:00Z', column: 'caps' },
    value: true,
  });
  const json = JSON.stringify(event);
  assert.doesNotMatch(json, /secret|Falcon|Approve/);
});

test('the Approve button and a message cell are described by message id and column only', () => {
  const { approve, textSpan } = reviewRow();
  assert.deepEqual(describeElement(approve), { tag: 'button', message_id: '3|2024-01-01T10:00:00Z', column: 'approve' });
  // A click on the message text lands on the row: described by the row's
  // message id and the cell's kind.
  assert.deepEqual(describeElement(textSpan), { tag: 'tr', message_id: '3|2024-01-01T10:00:00Z', column: 'msg-text' });
  assert.doesNotMatch(JSON.stringify(clickEvent(textSpan)), /secret/);
});

test('items in the timeline and lists are described by kind and position, not text', () => {
  const bar = el('div', { className: 'bar', attrs: { 'data-block-idx': '12' } }, el('span', { text: 'Project Falcon' }));
  const flag = el('span', { className: 'flag-icon critical', text: '⚑', attrs: { 'data-flag-type': 'critical' } });
  const day = el('div', { className: 'day-label', text: '3 Wed', attrs: { 'data-day': '2024-01-03' } });
  el('div', { id: 'calendarBody' }, bar, flag, day);
  const conv = el('div', { className: 'conv-item', text: 'Project Falcon', attrs: { 'data-idx': '4' } });
  el('div', { id: 'convItems' }, conv);
  const plain = el('em', { text: 'Project Falcon' });
  el('div', { id: 'analyticsMain' }, plain);

  assert.deepEqual(describeElement(bar.children[0]), { tag: 'div', column: 'bar:12' });
  assert.deepEqual(describeElement(flag), { tag: 'span', column: 'critical' });
  assert.deepEqual(describeElement(day), { tag: 'div', column: 'day-label:2024-01-03' });
  assert.deepEqual(describeElement(conv), { tag: 'div', column: 'conv-item:4' });
  assert.deepEqual(describeElement(plain), { tag: 'em' });
});

test('a button with an id inside a banner keeps its id but not its text', () => {
  const btn = el('button', { id: 'viewEntireConvBtn', className: 'btn-secondary', text: 'View Project Falcon' });
  el('div', { id: 'reviewFilterBanner' }, btn);
  assert.deepEqual(describeElement(btn), { tag: 'button', id: 'viewEntireConvBtn', column: 'btn-secondary' });
});

test('attributes are capped and stripped of control and invisible characters', () => {
  const long = el('button', { id: 'A'.repeat(200) });
  assert.equal(describeElement(long).id.length, TEXT_CAP);
  const sneaky = el('button', { id: 'Sa\u200Bve\u202E\u0000 now\tplease\u00AD' });
  assert.equal(describeElement(sneaky).id, 'Save now please');
  assert.equal(cleanText('😀'.repeat(100)).length, TEXT_CAP * 2); // 80 emoji, never half of one
  assert.equal(cleanText(' two  spaces '), 'two spaces');
  assert.equal(cleanText('plain words'), 'plain words');
  assert.equal(cleanText(null), '');
  assert.equal(cleanText(42), '');
  assert.equal(cleanText('abcdef', 3), 'abc');
});

test('an element that is not part of the page tree is still described', () => {
  assert.deepEqual(describeElement({ tagName: 'HTML' }), { tag: 'html' });
  assert.deepEqual(describeElement({}), { tag: '' });
});

test('changes: checkbox and select values, file size and extension only', () => {
  const select = el('select', { id: 'reviewFilter', value: 'critical' });
  assert.deepEqual(changeEvent(select), { kind: 'change', target: { tag: 'select', id: 'reviewFilter' }, value: 'critical' });
  const radio = el('input', { type: 'radio', checked: false });
  assert.equal(changeEvent(radio).value, false);

  const file = el('input', { id: 'loadConvFile', type: 'file', files: [{ name: 'Alice Smith conversations.JSON', size: 60600000 }] });
  const event = changeEvent(file);
  assert.deepEqual(event, { kind: 'change', target: { tag: 'input', id: 'loadConvFile' }, file_size: 60600000, file_ext: 'json' });
  assert.doesNotMatch(JSON.stringify(event), /Alice/);

  const noExt = el('input', { type: 'file', files: [{ name: 'export', size: 'x' }] });
  assert.deepEqual(changeEvent(noExt).file_ext, '');
  assert.equal(changeEvent(noExt).file_size, 0);
  const cleared = el('input', { type: 'file', files: [] });
  assert.deepEqual(changeEvent(cleared), { kind: 'change', target: { tag: 'input' }, file_size: 0, file_ext: '' });
});

test("a text box's change is not recorded: its text is recorded only on submit", () => {
  const search = el('input', { id: 'reviewSearch', type: 'search', value: MESSAGE_TEXT });
  assert.equal(changeEvent(search), null);
  assert.equal(changeEvent(el('textarea', { value: 'x' })), null);
});

test('which elements are text boxes', () => {
  for(const type of ['text', 'search', 'email', '', 'number']) assert.equal(isTextBox(el('input', { type })), true, type);
  assert.equal(isTextBox(el('input', {})), true);
  assert.equal(isTextBox(el('textarea')), true);
  for(const type of ['checkbox', 'file', 'button']) assert.equal(isTextBox(el('input', { type })), false, type);
  assert.equal(isTextBox(el('select')), false);
});

test('a submit carries the text box and its capped text', () => {
  const box = el('input', { id: 'convSearch', type: 'search' });
  assert.deepEqual(submitEvent(box, ' falcon​ '), { kind: 'submit', target: { tag: 'input', id: 'convSearch' }, text: 'falcon' });
  assert.equal(submitEvent(box, 'x'.repeat(500)).text.length, TEXT_CAP);
});

test('view and shown events', () => {
  assert.deepEqual(viewEvent('review', 'router'), { kind: 'view', view: 'review', via: 'router' });
  assert.deepEqual(shownEvent('saveStatus', 'save.saved', 0), { kind: 'shown', where: 'saveStatus', message: 'save.saved', is_error: false });
  assert.deepEqual([...SHOWN_PLACES], [
    'loadStatus', 'saveStatus', 'loadProgress', 'signIn', 'signInStatus', 'fileFailures', 'describeStatus',
    'describeReminder',
  ]);
});

test('routes replace every UUID with {id} and drop the query', () => {
  assert.equal(routeTemplate('/uploads/7f3e0b4c-1d2e-4f5a-8b9c-0d1e2f3a4b5c'), '/uploads/{id}');
  assert.equal(
    routeTemplate('/conversations/7F3E0B4C-1D2E-4F5A-8B9C-0D1E2F3A4B5C/messages/00000000-1111-4111-8111-111111111111/flags?x=1#y'),
    '/conversations/{id}/messages/{id}/flags');
  assert.equal(routeTemplate('/detect'), '/detect');
  assert.equal(S3_UPLOAD_ROUTE, 's3 PUT raw/…');
  assert.equal(S3_EXPORT_ROUTE, 's3 GET export/…');
});

test('request events: answered, unanswered, with facts', () => {
  assert.deepEqual(requestEvent({
    t: 1000, method: 'POST', route: '/detect', status: 200, ms: 3210.6, bytes: 120, requestId: 'abc=', facts: { offset: 0, limit: 5 },
  }), { kind: 'request', t: 1000, method: 'POST', route: '/detect', status: 200, ms: 3211, request_id: 'abc=', bytes: 120, offset: 0, limit: 5 });
  assert.deepEqual(requestEvent({
    t: 5, method: 'PUT', route: S3_UPLOAD_ROUTE, status: null, ms: -1, requestId: null, errorKind: 'network',
  }), { kind: 'request', t: 5, method: 'PUT', route: S3_UPLOAD_ROUTE, status: null, ms: 0, request_id: null, error_kind: 'network' });
  assert.equal(requestEvent({ t: 5, method: 'GET', route: '/x', status: null, ms: 1, errorKind: 'oops: secret' }).error_kind, 'other');
});

test('a password box is never a text box, so Enter in it records nothing', () => {
  assert.equal(isTextBox(el('input', { type: 'password' })), false);
  assert.equal(isTextBox(el('input', { type: 'PASSWORD' })), false);
});

test('a shown event keeps only numbers and a known error kind from its values', () => {
  assert.deepEqual(
    shownEvent('loadStatus', 'load.failed', true, { status: 503, error_kind: 'server_error', detail: 'secret server text' }),
    { kind: 'shown', where: 'loadStatus', message: 'load.failed', is_error: true, status: 503, error_kind: 'server_error' });
  assert.deepEqual(
    shownEvent('loadProgress', 'wait.retrying', false, { attempt: 2, max_attempts: 3, count: NaN, error_kind: 'made up' }),
    { kind: 'shown', where: 'loadProgress', message: 'wait.retrying', is_error: false, attempt: 2, max_attempts: 3 });
});

test('a message cell whose class has a leading space still names its column', () => {
  const span = el('span', { text: 'x' });
  el('div', { id: 'reviewTable' }, el('tr', { attrs: { 'data-msg-id': '1|t' } }, el('td', { className: ' msg-text when' }, span)));
  assert.deepEqual(describeElement(span), { tag: 'tr', message_id: '1|t', column: 'msg-text' });
});
