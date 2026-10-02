import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  LABEL_CAP, S3_EXPORT_ROUTE, S3_UPLOAD_ROUTE, SHOWN_PLACES, SHOWN_TEXT_CAP,
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

test('an element is described by its id and its label', () => {
  const button = el('button', { id: 'loadBtn', text: '  Load\n ' });
  assert.deepEqual(describeElement(button), { tag: 'button', id: 'loadBtn', label: 'Load' });
});

test('a click on a span inside a button describes the button', () => {
  const inner = el('span', { text: 'Review' });
  el('nav', {}, el('button', { attrs: { 'data-tab': 'review' } }, inner));
  assert.deepEqual(clickEvent(inner), { kind: 'click', target: { tag: 'button', label: 'Review' } });
});

test("a checkbox is labelled by its label's text, an aria-label wins over text", () => {
  const box = el('input', { id: 'autoDetectCheckbox', type: 'checkbox', checked: false });
  el('label', { text: 'Scan for flags ' }, box);
  assert.deepEqual(describeElement(box), { tag: 'input', id: 'autoDetectCheckbox', label: 'Scan for flags' });
  const loose = el('input', { type: 'checkbox' });
  assert.deepEqual(describeElement(loose), { tag: 'input' });
  const named = el('button', { text: 'x', attrs: { 'aria-label': 'Close' } });
  assert.deepEqual(describeElement(named), { tag: 'button', label: 'Close' });
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

test('labels are capped and stripped of control and invisible characters', () => {
  const long = el('button', { text: 'A'.repeat(200) });
  assert.equal(describeElement(long).label.length, LABEL_CAP);
  const sneaky = el('button', { text: 'Sa​ve‮\u0000 now\tplease­' });
  assert.equal(describeElement(sneaky).label, 'Save now please');
  assert.equal(cleanText('😀'.repeat(100)).length, LABEL_CAP * 2); // 80 emoji, never half of one
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
  assert.equal(submitEvent(box, 'x'.repeat(500)).text.length, LABEL_CAP);
});

test('view and shown events', () => {
  assert.deepEqual(viewEvent('review', 'router'), { kind: 'view', view: 'review', via: 'router' });
  assert.deepEqual(shownEvent('saveStatus', 'Saved.', 0), { kind: 'shown', where: 'saveStatus', text: 'Saved.', is_error: false });
  assert.equal(shownEvent('error', 'e'.repeat(500), true).text.length, SHOWN_TEXT_CAP);
  assert.deepEqual([...SHOWN_PLACES], ['loadStatus', 'saveStatus', 'loadProgress', 'restoredNotice', 'signIn', 'error']);
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
    t: 5, method: 'PUT', route: S3_UPLOAD_ROUTE, status: null, ms: -1, requestId: null, error: 'Failed to fetch',
  }), { kind: 'request', t: 5, method: 'PUT', route: S3_UPLOAD_ROUTE, status: null, ms: 0, request_id: null, error: 'Failed to fetch' });
});

test('a password box is never a text box, so Enter in it records nothing', () => {
  assert.equal(isTextBox(el('input', { type: 'password' })), false);
  assert.equal(isTextBox(el('input', { type: 'PASSWORD' })), false);
});
