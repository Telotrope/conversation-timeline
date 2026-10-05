// The listeners that notice what you do on the page, for the activity log
// (plan docs/plans/completed/2026-10-02-activity-instrumentation.md §4). One listener
// per kind of action, in the capture phase on the window or document, so
// they run just before the page's own handlers and never change what those
// handlers do.
//
// Recorded: every click; every change of a checkbox, selector or the file
// chooser; a text box's contents only when you submit it (Enter in it, or a
// form's own submit), never as you type; and each change of the address's
// `#` part, which selects the tab. When the page is hidden or closed, what
// is waiting is sent (`leave`).
//
// The kinds of event listened for here are what tests/activity-inventory.test.js
// checks the rest of the page against.

import { changeEvent, clickEvent, isTextBox, submitEvent, viewEvent } from '../core/activity-event.js';

// An Enter in a text box inside a form also makes the form submit; the
// form's submit within this long after is the same action, not a second one.
const SAME_SUBMIT_MS = 1000;

// A listener that fails must never break the page: the failure is reported
// in the console and the page's own handlers run as usual.
function guarded(warn, handler){
  return (e) => {
    try{
      handler(e);
    } catch(err){
      warn(`activity recording failed (${e && e.type}): ${err && err.message}`);
    }
  };
}

// win, doc: the window and document. record(event): adds one record.
// leave(): sends what is waiting, the page being hidden or closed.
// now(): the clock, in milliseconds. warn(message): reports a failure.
export function installActivityListeners({ win, doc, record, leave, now, warn }){
  let lastEnter = null;
  const capture = { capture: true };

  doc.addEventListener('click', guarded(warn, (e) => record(clickEvent(e.target))), capture);

  doc.addEventListener('change', guarded(warn, (e) => {
    const event = changeEvent(e.target);
    if(event) record(event);
  }), capture);

  doc.addEventListener('keydown', guarded(warn, (e) => {
    if(e.key !== 'Enter' || e.isComposing || !isTextBox(e.target)) return;
    lastEnter = { el: e.target, at: now() };
    record(submitEvent(e.target, e.target.value));
  }), capture);

  doc.addEventListener('submit', guarded(warn, (e) => {
    const form = e.target;
    if(lastEnter && form.contains(lastEnter.el) && now() - lastEnter.at < SAME_SUBMIT_MS) return;
    const active = doc.activeElement;
    const text = active && form.contains(active) && isTextBox(active) ? active.value : '';
    record(submitEvent(form, text));
  }), capture);

  win.addEventListener('hashchange', guarded(warn, () => {
    const tab = String(win.location.hash).replace(/^#/, '').split('/')[0];
    record(viewEvent(tab, 'hashchange'));
  }), capture);

  win.addEventListener('pagehide', guarded(warn, () => leave()));
  doc.addEventListener('visibilitychange', guarded(warn, () => {
    if(doc.visibilityState === 'hidden') leave();
  }));
}
