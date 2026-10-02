// Builds the records of what you did on the page, for the activity log
// (plan docs/plans/2026-10-02-activity-instrumentation.md §4). Pure: it
// reads only the element or values it is handed, never the page itself.
//
// What an element description may hold is deliberately narrow: its tag, its
// `id`, a short label (a button's text, a checkbox's label, a tab's name),
// and, inside the parts of the page that show your conversations, a message
// id and a column. Never text from table cells, message bodies, file names,
// file contents or tokens: those parts of the page are described by where
// the element is, not by what it says.
//
// Every string goes through cleanText: whitespace runs become one space,
// control and invisible characters (zero-width, right-to-left overrides,
// byte-order marks, NUL) are removed, and the result is capped.

export const LABEL_CAP = 80;
export const SHOWN_TEXT_CAP = 200;

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

// Cleans and caps a piece of text. Not a string (null, a number) gives ''.
// The cap counts characters, not UTF-16 units, so it never splits an emoji.
export function cleanText(value, cap = LABEL_CAP){
  if(typeof value !== 'string') return '';
  const cleaned = value.slice(0, cap * 4).replace(WHITESPACE, ' ').replace(INVISIBLE, '').trim();
  // Most text is already short enough: splitting it into characters only
  // when it might be too long keeps a click's recording cheap.
  if(cleaned.length <= cap) return cleaned;
  return Array.from(cleaned).slice(0, cap).join('');
}

function attr(el, name){
  return el.getAttribute ? el.getAttribute(name) : null;
}

function tagOf(el){
  return cleanText(String(el.tagName || '').toLowerCase(), 20);
}

// A control's visible name, for elements outside the conversation regions.
// Only controls get one: a plain <div> or <p> can hold any text at all.
function labelOf(el){
  const tag = tagOf(el);
  const aria = attr(el, 'aria-label');
  if(aria) return cleanText(aria);
  if(tag === 'button' || tag === 'a' || tag === 'label') return cleanText(el.textContent);
  if(tag === 'input' && (el.type === 'checkbox' || el.type === 'radio')){
    const label = el.closest ? el.closest('label') : null;
    return label ? cleanText(label.textContent) : '';
  }
  return '';
}

// Where in a conversation region an element is, without its text: the flag
// column a checkbox belongs to, the Approve button, a table cell's kind, or
// an item's kind with its position (a conversation's index, a session's
// number, a day). `target` is the exact element under the pointer, whose
// table cell names the column when `el` is the whole row.
function columnOf(el, target){
  if(el.dataset && el.dataset.type) return cleanText(el.dataset.type, 40);
  if(el.dataset && el.dataset.flagType) return cleanText(el.dataset.flagType, 40);
  const classes = String(el.className || '').split(' ').filter(Boolean);
  if(classes.includes('approve-btn')) return 'approve';
  const cell = target.closest ? target.closest('td') : null;
  const kind = (cell && cell !== el && cell.className) || classes[0] || '';
  const position = el.dataset
    ? el.dataset.idx ?? el.dataset.blockIdx ?? el.dataset.day ?? el.dataset.analysis
    : undefined;
  const name = String(kind).split(' ')[0];
  return cleanText(position === undefined ? name : `${name}:${position}`, 40);
}

function messageIdOf(el){
  const own = attr(el, 'data-id');
  if(own) return cleanText(own);
  const row = el.closest ? el.closest('[data-msg-id]') : null;
  return row ? cleanText(attr(row, 'data-msg-id')) : '';
}

// Describes an element for a click, change or submit record:
// { tag, id?, label?, message_id?, column? }, absent keys left out.
export function describeElement(target){
  const el = (target.closest && target.closest(ACTIONABLE)) || target;
  const out = { tag: tagOf(el) };
  const id = cleanText(el.id || '');
  if(id) out.id = id;
  const inContent = el.closest ? el.closest(CONTENT_REGIONS) !== null : false;
  if(inContent){
    const messageId = messageIdOf(el);
    if(messageId) out.message_id = messageId;
    const column = columnOf(el, target);
    if(column) out.column = column;
  } else {
    const label = labelOf(el);
    if(label) out.label = label;
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

export function isTextBox(el){
  const tag = tagOf(el);
  if(tag === 'textarea') return true;
  return tag === 'input' && TEXT_INPUT_TYPES.has(String(el.type || '').toLowerCase());
}

// A change to a checkbox, a selector or the file chooser; null for anything
// else (a text box's change is its typed text, recorded only on submit).
// The file chooser gives the file's size and extension only: its name can
// be personal.
export function changeEvent(target){
  const tag = tagOf(target);
  const type = String(target.type || '').toLowerCase();
  const base = { kind: 'change', target: describeElement(target) };
  if(tag === 'input' && (type === 'checkbox' || type === 'radio')) return { ...base, value: Boolean(target.checked) };
  if(tag === 'select') return { ...base, value: cleanText(String(target.value)) };
  if(tag === 'input' && type === 'file'){
    const file = target.files && target.files[0];
    if(!file) return { ...base, file_size: 0, file_ext: '' };
    const name = String(file.name || '');
    const dot = name.lastIndexOf('.');
    const ext = dot > 0 ? cleanText(name.slice(dot + 1).toLowerCase(), 10) : '';
    return { ...base, file_size: Number(file.size) || 0, file_ext: ext };
  }
  return null;
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
export const SHOWN_PLACES = Object.freeze(['loadStatus', 'saveStatus', 'loadProgress', 'restoredNotice', 'signIn', 'error']);

export function shownEvent(where, text, isError){
  return { kind: 'shown', where, text: cleanText(text, SHOWN_TEXT_CAP), is_error: Boolean(isError) };
}

const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi;

// An API path as a route: query string removed, every UUID replaced by {id}
// ("/uploads/7f3e…" -> "/uploads/{id}").
export function routeTemplate(path){
  const bare = String(path).split(/[?#]/)[0];
  return cleanText(bare.replace(UUID, '{id}'), LABEL_CAP);
}

// The route recorded for a signed link to S3: only the operation and the key's
// top folder, never the key or the signature in the link's query string.
export const S3_UPLOAD_ROUTE = 's3 PUT raw/…';
export const S3_EXPORT_ROUTE = 's3 GET export/…';

// One request the page made. status is null when no answer came back, and
// error then says why. facts: route-specific extras ({offset, limit} for
// /detect, {scan} for /uploads).
export function requestEvent({ t, method, route, status, ms, bytes, requestId, error, facts }){
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
  if(error !== undefined) event.error = cleanText(error, SHOWN_TEXT_CAP);
  return { ...event, ...facts };
}
