// Builds the records of what you did on the page, for the activity log
// (plan docs/plans/completed/2026-10-02-activity-instrumentation.md §4). Pure: it
// reads only the element or values it is handed, never the page itself.
//
// No wording from the page goes into a record (§4, C17): not button text,
// labels, messages, table cells, message bodies, file names or contents,
// tokens, or server error text. An element is described by its tag and its
// stable attributes only (`id`, `name`, `data-tab`, `data-analysis`), and,
// inside the parts of the page that show your conversations, by a message
// id and a column. A message the page shows is recorded by a fixed
// identifier; what it said is found from the page's code at the recorded
// page version. The one exception is a text box's contents when you submit
// it, which is what you typed, not the page's wording.
//
// Recording runs inside every click, so the click path avoids making
// throwaway objects and strings where it can (plan C18): text that is
// already clean is used as is, and tag names are looked up, not rebuilt.
//
// Every string from the page goes through cleanText: whitespace runs become
// one space, control and invisible characters (zero-width, right-to-left
// overrides, byte-order marks, NUL) are removed, and the result is capped.

import { ERROR_KINDS } from './page-error.js';

export const TEXT_CAP = 80;

// The parts of the page whose text comes from your conversations: the review
// table, the calendar's timeline, the conversation list and transcript, the
// analysis results, and the two banners that can name a conversation or the
// signed-in account.
const CONTENT_REGIONS =
  '#reviewTable, #calendarBody, #convItems, #convDetail, #analyticsMain, #reviewFilterBanner, #restoredNotice';

// The element a click is really "on": the nearest control or item above the
// exact thing under the pointer (often a <span> inside a button).
const ACTIONABLE =
  'button, a, input, select, textarea, label, [data-id], [data-msg-id], [data-idx], [data-block-idx], [data-flag-type], [data-day], [data-tab], [data-analysis]';

const WHITESPACE = /\s+/g;
// Cc: control characters (NUL, escape, ...); Cf: invisible formatting
// characters (zero-width space and joiner, right-to-left override, byte-order
// mark, soft hyphen).
const INVISIBLE = /[\p{Cc}\p{Cf}]/gu;
// Text that cleaning would change: anything but printable ASCII and single
// spaces between words.
const NEEDS_CLEANING = /[^\x21-\x7e ]| {2}|^ | $/;

// Cleans and caps a piece of text. Not a string (null, a number) gives ''.
// The cap counts characters, not UTF-16 units, so it never splits an emoji.
export function cleanText(value, cap = TEXT_CAP){
  if(typeof value !== 'string') return '';
  if(value.length <= cap && !NEEDS_CLEANING.test(value)) return value;
  const cleaned = value.slice(0, cap * 4).replace(WHITESPACE, ' ').replace(INVISIBLE, '').trim();
  if(cleaned.length <= cap) return cleaned;
  return Array.from(cleaned).slice(0, cap).join('');
}

function attr(el, name){
  return el.getAttribute ? el.getAttribute(name) : null;
}

// Lower-case tag names, made once per tag rather than once per click.
const TAGS = new Map();

function tagOf(el){
  const name = el.tagName;
  if(typeof name !== 'string') return '';
  let tag = TAGS.get(name);
  if(tag === undefined){
    tag = cleanText(name.toLowerCase(), 20);
    TAGS.set(name, tag);
  }
  return tag;
}

function firstWord(text){
  const start = text.trimStart();
  const space = start.indexOf(' ');
  return space < 0 ? start : start.slice(0, space);
}

const APPROVE = /(?:^|\s)approve-btn(?:\s|$)/;

// Where in a conversation region an element is, without its text: the flag
// column a checkbox belongs to, the Approve button, a table cell's kind, or
// an item's kind with its position (a conversation's index, a session's
// number, a day). `target` is the exact element under the pointer, whose
// table cell names the column when `el` is the whole row.
function columnOf(el, target){
  const type = attr(el, 'data-type');
  if(type) return cleanText(type, 40);
  const flagType = attr(el, 'data-flag-type');
  if(flagType) return cleanText(flagType, 40);
  const classes = typeof el.className === 'string' ? el.className : '';
  if(APPROVE.test(classes)) return 'approve';
  const cell = target.closest ? target.closest('td') : null;
  const kind = firstWord(cell && cell !== el && cell.className ? cell.className : classes);
  const position = attr(el, 'data-idx') ?? attr(el, 'data-block-idx') ?? attr(el, 'data-day') ?? attr(el, 'data-analysis');
  return cleanText(position === null ? kind : `${kind}:${position}`, 40);
}

function messageIdOf(el){
  const own = attr(el, 'data-id');
  if(own) return cleanText(own);
  const row = el.closest ? el.closest('[data-msg-id]') : null;
  return row ? cleanText(attr(row, 'data-msg-id')) : '';
}

// Describes an element for a click, change or submit record:
// { tag, id?, name?, tab?, analysis?, message_id?, column? }, absent keys
// left out. `tab` and `analysis` are the element's data-tab and
// data-analysis (a tab button, an analysis button).
export function describeElement(target){
  const el = (target.closest && target.closest(ACTIONABLE)) || target;
  const out = { tag: tagOf(el) };
  if(el.id) out.id = cleanText(el.id);
  const name = attr(el, 'name');
  if(name) out.name = cleanText(name);
  const tab = attr(el, 'data-tab');
  if(tab) out.tab = cleanText(tab);
  const analysis = attr(el, 'data-analysis');
  if(analysis) out.analysis = cleanText(analysis);
  if(el.closest && el.closest(CONTENT_REGIONS) !== null){
    const messageId = messageIdOf(el);
    if(messageId) out.message_id = messageId;
    const column = columnOf(el, target);
    if(column) out.column = column;
  }
  return out;
}

export function clickEvent(target){
  return { kind: 'click', target: describeElement(target) };
}

// Kinds of <input> whose value is free text you typed. Their value is only
// ever recorded on submit (submitEvent), never on change. Password boxes are
// deliberately not among them, so a password is never recorded.
const TEXT_INPUT_TYPES = new Set(['text', 'search', 'email', 'url', 'tel', 'number', '']);

function typeOf(el){
  return typeof el.type === 'string' ? el.type.toLowerCase() : '';
}

export function isTextBox(el){
  const tag = tagOf(el);
  if(tag === 'textarea') return true;
  return tag === 'input' && TEXT_INPUT_TYPES.has(typeOf(el));
}

// A change to a checkbox, a selector or the file chooser; null for anything
// else (a text box's change is its typed text, recorded only on submit).
// The file chooser gives the file's size and extension only: its name can
// be personal.
export function changeEvent(target){
  const tag = tagOf(target);
  const type = typeOf(target);
  let event = null;
  if(tag === 'input' && (type === 'checkbox' || type === 'radio')){
    event = { kind: 'change', target: describeElement(target), value: Boolean(target.checked) };
  } else if(tag === 'select'){
    event = { kind: 'change', target: describeElement(target), value: cleanText(String(target.value)) };
  } else if(tag === 'input' && type === 'file'){
    event = { kind: 'change', target: describeElement(target), file_size: 0, file_ext: '' };
    const file = target.files && target.files[0];
    if(file){
      const name = String(file.name || '');
      const dot = name.lastIndexOf('.');
      event.file_size = Number(file.size) || 0;
      event.file_ext = dot > 0 ? cleanText(name.slice(dot + 1).toLowerCase(), 10) : '';
    }
  }
  return event;
}

// What a text box held when you submitted it (Enter, or a form's submit).
export function submitEvent(target, text){
  return { kind: 'submit', target: describeElement(target), text: cleanText(text) };
}

// via: 'hashchange' (the address changed) or 'router' (the page opened a
// tab from the address).
export function viewEvent(view, via){
  return { kind: 'view', view: cleanText(view), via };
}

// The places the page shows a message; see the activity contract.
export const SHOWN_PLACES = Object.freeze(['loadStatus', 'saveStatus', 'loadProgress', 'restoredNotice', 'signIn']);

// A message the page showed, by its identifier (ui/widgets/page-messages.js).
// facts: the message's live values; only numbers (`count`, `attempt`,
// `max_attempts`, `status`) and a known `error_kind` are kept, so wording
// passed by mistake never reaches the record.
export function shownEvent(where, message, isError, facts){
  const event = { kind: 'shown', where, message, is_error: Boolean(isError) };
  if(facts){
    for(const key of Object.keys(facts)){
      const value = facts[key];
      if(key === 'error_kind'){
        if(ERROR_KINDS.includes(value)) event.error_kind = value;
      } else if(typeof value === 'number' && Number.isFinite(value)){
        event[key] = value;
      }
    }
  }
  return event;
}

const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;

// An API path as a route: query string removed, every UUID replaced by {id}
// ("/uploads/7f3e…" -> "/uploads/{id}").
export function routeTemplate(path){
  const bare = String(path).split(/[?#]/)[0];
  return cleanText(bare.replace(UUID, '{id}'), TEXT_CAP);
}

// The route recorded for a signed link to S3: only the operation and the key's
// top folder, never the key or the signature in the link's query string.
export const S3_UPLOAD_ROUTE = 's3 PUT raw/…';
export const S3_EXPORT_ROUTE = 's3 GET export/…';

// One request the page made. status is null when no answer came back, and
// errorKind (one of core/page-error.js's ERROR_KINDS) then says what kind
// of failure it was; never the error's message. facts: route-specific
// extras ({offset, limit} for /detect, {scan} for /uploads).
export function requestEvent({ t, method, route, status, ms, bytes, requestId, errorKind, facts }){
  const event = {
    kind: 'request',
    t,
    method: cleanText(method, 10),
    route,
    status: Number.isInteger(status) ? status : null,
    ms: Math.max(0, Math.round(ms)),
    request_id: requestId ? cleanText(requestId) : null,
  };
  if(Number.isInteger(bytes)) event.bytes = bytes;
  if(errorKind !== undefined) event.error_kind = ERROR_KINDS.includes(errorKind) ? errorKind : 'other';
  return Object.assign(event, facts);
}
