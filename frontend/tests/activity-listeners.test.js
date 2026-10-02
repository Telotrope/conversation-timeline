// The page's activity listeners (ui/activity-listeners.js), driven with a
// fake window and document: what each kind of action records, and that
// typing records nothing.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { installActivityListeners } from '../ui/activity-listeners.js';
import { el } from './fake-dom.js';

// A stand-in for the window or document: keeps each listener by kind, so a
// test can fire one with the event it wants.
function target(extra = {}){
  const listeners = {};
  return {
    ...extra,
    listeners,
    addEventListener(kind, fn, options){ (listeners[kind] ||= []).push({ fn, options }); },
    fire(kind, event = {}){ for(const l of listeners[kind] || []) l.fn({ type: kind, ...event }); },
  };
}

function install(){
  const h = { records: [], leaves: 0, warnings: [], clock: 0 };
  h.win = target({ location: { hash: '' } });
  h.doc = target({ activeElement: null, visibilityState: 'visible' });
  installActivityListeners({
    win: h.win,
    doc: h.doc,
    record: (event) => h.records.push(event),
    leave: () => { h.leaves += 1; },
    now: () => h.clock,
    warn: (message) => h.warnings.push(message),
  });
  return h;
}

test('the listeners are on the document and window, in the capture phase', () => {
  const h = install();
  assert.deepEqual(Object.keys(h.doc.listeners).sort(), ['change', 'click', 'keydown', 'submit', 'visibilitychange']);
  assert.deepEqual(Object.keys(h.win.listeners).sort(), ['hashchange', 'pagehide']);
  for(const kind of ['click', 'change', 'keydown', 'submit']) assert.equal(h.doc.listeners[kind][0].options.capture, true, kind);
  assert.equal(h.win.listeners.hashchange[0].options.capture, true);
});

test('a click is recorded with its element', () => {
  const h = install();
  h.doc.fire('click', { target: el('button', { id: 'loadBtn', text: 'Load' }) });
  assert.deepEqual(h.records, [{ kind: 'click', target: { tag: 'button', id: 'loadBtn' } }]);
});

test("a checkbox's change is recorded; a text box's change is not", () => {
  const h = install();
  h.doc.fire('change', { target: el('input', { id: 'toggleShowReplies', type: 'checkbox', checked: true }) });
  h.doc.fire('change', { target: el('input', { id: 'reviewSearch', type: 'search', value: 'secret words' }) });
  assert.deepEqual(h.records, [{ kind: 'change', target: { tag: 'input', id: 'toggleShowReplies' }, value: true }]);
});

test('typing in a text box records nothing; Enter in it records one submit with its text', () => {
  const h = install();
  const box = el('input', { id: 'convSearch', type: 'search', value: '' });
  for(const key of ['f', 'a', 'l', 'c', 'o', 'n', 'Backspace', 'Shift']){
    box.value += key.length === 1 ? key : '';
    h.doc.fire('keydown', { key, target: box });
    h.doc.fire('input', { target: box });
  }
  assert.deepEqual(h.records, []);
  h.doc.fire('keydown', { key: 'Enter', target: box });
  assert.deepEqual(h.records, [{ kind: 'submit', target: { tag: 'input', id: 'convSearch' }, text: 'falcon' }]);
});

test('Enter elsewhere, or while composing a character, records nothing', () => {
  const h = install();
  h.doc.fire('keydown', { key: 'Enter', target: el('button', { id: 'loadBtn' }) });
  h.doc.fire('keydown', { key: 'Enter', target: el('input', { type: 'checkbox' }) });
  h.doc.fire('keydown', { key: 'Enter', isComposing: true, target: el('input', { type: 'text', value: 'か' }) });
  assert.deepEqual(h.records, []);
});

test("a form's submit is recorded with its focused text box, once when it follows Enter", () => {
  const h = install();
  const box = el('input', { id: 'q', type: 'text', value: 'falcon' });
  const form = el('form', { id: 'searchForm' }, box);
  h.doc.activeElement = box;

  h.doc.fire('keydown', { key: 'Enter', target: box });
  h.clock += 10;
  h.doc.fire('submit', { target: form });
  assert.equal(h.records.length, 1, 'the Enter and the submit it causes are one action');

  h.clock += 5000;
  h.doc.fire('submit', { target: form });
  assert.deepEqual(h.records[1], { kind: 'submit', target: { tag: 'form', id: 'searchForm' }, text: 'falcon' });

  h.doc.activeElement = el('button');
  h.doc.fire('submit', { target: form });
  assert.equal(h.records[2].text, '');
  h.doc.activeElement = null;
  h.doc.fire('submit', { target: el('form') });
  assert.equal(h.records[3].text, '');
});

test('an address change records the tab it names', () => {
  const h = install();
  h.win.location.hash = '#conversations/3';
  h.win.fire('hashchange');
  h.win.location.hash = '';
  h.win.fire('hashchange');
  assert.deepEqual(h.records, [
    { kind: 'view', view: 'conversations', via: 'hashchange' },
    { kind: 'view', view: '', via: 'hashchange' },
  ]);
});

test('hiding or closing the page sends what waits; showing it again does not', () => {
  const h = install();
  h.win.fire('pagehide');
  h.doc.visibilityState = 'hidden';
  h.doc.fire('visibilitychange');
  h.doc.visibilityState = 'visible';
  h.doc.fire('visibilitychange');
  assert.equal(h.leaves, 2);
});

test('a listener that fails warns and never throws into the page', () => {
  const h = install();
  const broken = { closest(){ throw new Error('detached'); } };
  assert.doesNotThrow(() => h.doc.fire('click', { target: broken }));
  assert.deepEqual(h.warnings, ['activity recording failed (click): detached']);
  assert.deepEqual(h.records, []);
});

test('Enter in a password box records nothing, so a password is never recorded', () => {
  const h = install();
  h.doc.fire('keydown', { key: 'Enter', target: el('input', { type: 'password', value: 'hunter2' }) });
  assert.deepEqual(h.records, []);
});
