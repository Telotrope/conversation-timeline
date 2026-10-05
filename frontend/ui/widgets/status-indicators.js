// The page's progress and status lines: the progress bar with its
// time-remaining estimate and the status line above it (on the Upload page,
// or borrowed by the loading modal), the per-file lines below the bar, the
// Sign-in and Describe pages' own lines, and the "Saved." / "Could not
// save" line.

import { formatBytes, formatEta } from '../../core/format.js';
import { waitMessageId } from '../../core/upload-wait.js';
import { shownEvent } from '../../core/activity-event.js';
import { recordActivity } from '../../core/activity-sink.js';
import { pageMessage, recordedValues } from './page-messages.js';

// Every setter here takes a message's identifier (page-messages.js) and its
// live values, shows the wording, and records the identifier for the
// activity log (plan docs/plans/completed/2026-10-02-activity-instrumentation.md §4,
// `shown`), never the wording.
function recordShown(where, id, entry, values){
  recordActivity(shownEvent(where, id, entry.isError, recordedValues(entry, values)));
}

// Shows message `id` on the status line `elementId`, red when it reports an
// error, and records it as shown at `where`. id null clears the line, which
// shows nothing and isn't recorded.
function setLine(elementId, where, id, values){
  const el = document.getElementById(elementId);
  if(id === null){
    el.textContent = '';
    el.classList.remove('is-error');
    return;
  }
  const entry = pageMessage(id);
  el.textContent = entry.text(values);
  el.classList.toggle('is-error', entry.isError);
  recordShown(where, id, entry, values);
}

// The status line above the progress bar.
export function setLoadStatus(id, values = {}){
  setLine('loadStatus', 'loadStatus', id, values);
}

// The Sign-in page's own line: a sign-in that ran out, or the server
// unreachable while checking for your conversations.
export function setSignInStatus(id, values = {}){
  setLine('signInStatus', 'signInStatus', id, values);
}

// The Describe page's line, beside Done.
export function setDescribeStatus(id, values = {}){
  setLine('describeStatus', 'describeStatus', id, values);
}

// One line per file below the Upload page's bar, for a file that failed
// or was stopped. clearFileLines empties the list.
export function addFileLine(id, values = {}){
  const entry = pageMessage(id);
  const li = document.createElement('li');
  li.textContent = entry.text(values);
  li.classList.toggle('is-error', entry.isError);
  document.getElementById('fileFailures').appendChild(li);
  recordShown('fileFailures', id, entry, values);
}

export function clearFileLines(){
  document.getElementById('fileFailures').replaceChildren();
}

// The Describe page's reminder of the batch's files that aren't there,
// each with why: [{ file, reason }]. Empty hides it.
export function showDescribeReminder(items){
  const box = document.getElementById('describeReminder');
  box.hidden = items.length === 0;
  if(items.length === 0){
    box.replaceChildren();
    return;
  }
  const entry = pageMessage('describe.reminder');
  const intro = document.createElement('span');
  intro.textContent = entry.text({ count: items.length });
  const list = document.createElement('ul');
  list.append(...items.map(({ file, reason }) => {
    const li = document.createElement('li');
    li.textContent = `${file}: ${reason}`;
    return li;
  }));
  box.replaceChildren(intro, list);
  recordShown('describeReminder', 'describe.reminder', entry, { count: items.length });
}

// --- Load-screen progress ---
// Three states, because the load has three genuinely different kinds of
// phase: a measurable transfer, an unmeasurable wait, and finished. Nothing
// here invents a percentage for work whose size isn't known.

export function showLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  const fill = document.getElementById('loadProgressFill');
  wrap.hidden = false;
  fill.classList.remove('is-error', 'is-working');
  fill.style.width = '0%';
  document.getElementById('loadProgressLabel').textContent = '';
}

export function hideLoadProgress(){
  document.getElementById('loadProgress').hidden = true;
}

export function failLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  if(wrap.hidden) return;
  const fill = document.getElementById('loadProgressFill');
  fill.classList.remove('is-working');
  fill.classList.add('is-error');
  recordShown('loadProgress', 'progress.failed', pageMessage('progress.failed'));
}

// A phase whose duration can't be observed: full-width track, no number.
// Used for the stretch after the request body is fully sent but before the
// server answers -- the local-dev PUT handler does its processing there, and
// a bar frozen at 100% would read as hung.
//
// The bar moves (CSS stripes, `.progress-fill.is-working`) so the wait
// reads as work in progress, not a bar frozen at 100% (plan
// 2026-10-02-upload-processing-failures.md §3). A measured transfer calls
// setLoadProgressMeasured first, which stops the stripes.
export function setLoadProgressIndeterminate(id){
  const entry = pageMessage(id);
  document.getElementById('loadProgress').hidden = false;
  const fill = document.getElementById('loadProgressFill');
  fill.classList.add('is-working');
  fill.style.width = '100%';
  document.getElementById('loadProgressLabel').textContent = entry.text({});
  lastLabelRecord = null;
  recordShown('loadProgress', id, entry);
}

// Before a transfer whose progress is measured: a plain bar again.
export function setLoadProgressMeasured(){
  document.getElementById('loadProgressFill').classList.remove('is-working');
}

// What setLoadProgressLabel last recorded, so a label that only changes its
// ticking clock (the wait for the server, rewritten every second) is
// recorded once, and again only when the message or its numbers change.
let lastLabelRecord = null;

// Replaces the label only, keeping the bar as it is.
export function setLoadProgressLabel(id, values = {}){
  const entry = pageMessage(id);
  document.getElementById('loadProgressLabel').textContent = entry.text(values);
  const key = `${id} ${JSON.stringify(recordedValues(entry, values) || {})}`;
  if(key === lastLabelRecord) return;
  lastLabelRecord = key;
  recordShown('loadProgress', id, entry, values);
}

// --- The text under the bar for each kind of progress ---
// One function per kind, so the load steps (ui/load-flow.js) report numbers
// through callbacks and never write to the page themselves.

function fillTo(pct){
  document.getElementById('loadProgressFill').style.width = pct + '%';
}

function labelText(text){
  document.getElementById('loadProgressLabel').textContent = text;
}

// Sending files: bytes so far of the total, and the time left when known.
export function showSendProgress(loaded, total, eta){
  const pct = Math.round((loaded / total) * 100);
  fillTo(pct);
  labelText(`Sending your file — ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : ''));
}

// Waiting for the server to process an upload: which attempt, why the last
// one failed, and a clock (core/upload-wait.js's describeWait).
export function showWaitProgress(answer, elapsedMs){
  setLoadProgressLabel(waitMessageId(answer), {
    answer, elapsedMs, attempt: answer.attempt, max_attempts: answer.max_attempts,
  });
}

// The scan: conversations covered so far of the total.
export function showScanProgress(covered, total){
  const pct = total ? Math.round((covered / total) * 100) : 100;
  fillTo(pct);
  labelText(`Scanning your messages — ${covered} of ${total} conversation${total === 1 ? '' : 's'} (${pct}%)`);
}

// Downloading the timeline: a share of the whole when its size is known,
// otherwise just what has arrived, rather than a made-up proportion.
export function showDownloadProgress(loaded, total, eta){
  if(total){
    const pct = Math.round((loaded / total) * 100);
    fillTo(pct);
    labelText(`Receiving your processed timeline — ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : ''));
  } else {
    fillTo(100);
    labelText(`Receiving your processed timeline — ${formatBytes(loaded)} so far`);
  }
}

// Estimates remaining time from a rolling window of recent progress events
// rather than an average over the whole transfer -- a whole-transfer average
// keeps reporting a stale rate long after the speed changes. Returns '' until
// there are at least two samples spanning enough time to mean anything.
export function makeRateEstimator(windowMs){
  const samples = [];
  return function(loaded, total){
    const now = Date.now();
    samples.push({ t: now, loaded });
    while(samples.length > 2 && now - samples[0].t > windowMs) samples.shift();
    if(samples.length < 2) return '';
    const first = samples[0];
    const elapsed = (now - first.t) / 1000;
    const moved = loaded - first.loaded;
    if(elapsed < 1 || moved <= 0) return '';
    return formatEta((total - loaded) / (moved / elapsed));
  };
}

// An error message here only marks the activity record; the line looks the
// same either way.
export function setSaveStatus(id, values = {}){
  const entry = pageMessage(id);
  const el = document.getElementById('saveStatus');
  if(el) el.textContent = entry.text(values);
  recordShown('saveStatus', id, entry, values);
}
