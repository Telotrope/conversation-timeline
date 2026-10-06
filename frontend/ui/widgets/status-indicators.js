// The page's progress and status lines: the progress bars, each with its
// clock (createProgressBar), the load screen's bar with its time-remaining
// estimate and the status line above it (on the Upload page, or borrowed by
// the loading modal), the per-file lines below the bar, the Sign-in and
// Describe pages' own lines, and the "Saved." / "Could not save" line.

import { formatEta } from '../../core/format.js';
import { processingProgress, waitMessageId } from '../../core/upload-wait.js';
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

// --- Progress bars ---
// Every wait shows a bar that moves only on real progress (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b): bytes sent
// or received, sessions done of the total, conversations written. Where
// nothing can be measured (inside one request, or before the first answer)
// the bar is striped at full width, which measures nothing and says so. In
// both, a clock the page keeps itself is shown beside the words, worded as
// time spent, not as work done: "… · 12 s so far". It ticks every second;
// the activity log records the label once, not each tick.

// The clock's default timers; a test hands in its own.
const TIMERS = {
  now: () => Date.now(),
  every: (fn, ms) => setInterval(fn, ms),
  cancel: (handle) => clearInterval(handle),
};

// A bar and its label. nodes() returns { fill, label }, looked up each time
// so a bar whose elements are drawn again still works; where: the place the
// activity log records its messages at (core/activity-event.js's
// SHOWN_PLACES), or null to record nothing.
export function createProgressBar({ nodes, where = null }, timers = TIMERS){
  let started = null;
  let ticking = null;
  let text = '';
  let withClock = true;
  let recorded = null;

  const draw = () => {
    const { label } = nodes();
    const seconds = started === null ? 0 : Math.floor((timers.now() - started) / 1000);
    label.textContent = withClock && seconds >= 1 ? `${text} · ${seconds} s so far` : text;
  };
  const startClock = () => {
    if(started !== null) return;
    started = timers.now();
    ticking = timers.every(draw, 1000);
  };
  const stop = () => {
    if(ticking !== null) timers.cancel(ticking);
    ticking = null;
    started = null;
  };
  const show = (id, values, clock) => {
    const entry = pageMessage(id);
    text = entry.text(values);
    withClock = clock;
    draw();
    const key = `${id} ${JSON.stringify(recordedValues(entry, values) || {})}`;
    if(where === null || key === recorded) return;
    recorded = key;
    recordShown(where, id, entry, values);
  };
  const fill = (classes, width) => {
    const el = nodes().fill;
    el.classList.remove('is-working', 'is-error');
    if(classes) el.classList.add(classes);
    el.style.width = width;
  };
  const fillTo = (done, total) => fill(null, `${total ? Math.min(100, Math.round((done / total) * 100)) : 100}%`);

  return {
    // A wait that can't be measured: a new clock, the bar striped, and the
    // message recorded even if it was the last one shown.
    working(id, values = {}){
      stop();
      fill('is-working', '100%');
      startClock();
      recorded = null;
      show(id, values, true);
    },
    // Real progress: `done` of `total`. The clock carries on from the wait's
    // start; the striped state is cleared.
    measured(id, values, done, total){
      fillTo(done, total);
      startClock();
      show(id, { ...values, done, total }, true);
    },
    // Moves the bar without changing the words.
    fillTo,
    // Words that carry their own clock (the processing wait's); the bar's
    // clock stops.
    label(id, values = {}){
      stop();
      show(id, values, false);
    },
    // Red, at the width it had reached; an empty bar fills, so the red
    // shows.
    failed(id, values = {}){
      stop();
      const width = nodes().fill.style.width;
      fill('is-error', width && width !== '0%' ? width : '100%');
      show(id, values, false);
    },
    stop,
    // Empty and still, ready for a new wait.
    reset(){
      stop();
      recorded = null;
      fill(null, '0%');
      text = '';
      nodes().label.textContent = '';
    },
  };
}

// --- Load-screen progress ---
// The bar under the status line, on the Upload page or borrowed by the
// loading modal.

let LOAD_BAR = null;

function loadBar(){
  if(!LOAD_BAR){
    LOAD_BAR = createProgressBar({
      nodes: () => ({ fill: document.getElementById('loadProgressFill'), label: document.getElementById('loadProgressLabel') }),
      where: 'loadProgress',
    });
  }
  return LOAD_BAR;
}

export function showLoadProgress(){
  document.getElementById('loadProgress').hidden = false;
  loadBar().reset();
}

export function hideLoadProgress(){
  loadBar().stop();
  document.getElementById('loadProgress').hidden = true;
}

export function failLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  if(wrap.hidden) return;
  loadBar().stop();
  const fill = document.getElementById('loadProgressFill');
  fill.classList.remove('is-working');
  fill.classList.add('is-error');
  recordShown('loadProgress', 'progress.failed', pageMessage('progress.failed'));
}

// A phase whose duration can't be observed: full-width striped track, no
// number, and the clock.
export function setLoadProgressIndeterminate(id){
  document.getElementById('loadProgress').hidden = false;
  loadBar().working(id);
}

// Real progress on the load bar: message `id` with `values`, `done` of
// `total`.
export function showLoadMeasured(id, values, done, total){
  document.getElementById('loadProgress').hidden = false;
  loadBar().measured(id, values, done, total);
}

// Replaces the label only, keeping the bar as it is: for words that carry
// their own clock. A label that only changes its ticking clock (the wait
// for the server, rewritten every second) is recorded once, and again only
// when the message or its numbers change.
export function setLoadProgressLabel(id, values = {}){
  loadBar().label(id, values);
}

// --- The text under the bar for each kind of progress ---
// One function per kind, so the load steps (ui/load-flow.js) report numbers
// through callbacks and never write to the page themselves.

// Preparing files in the browser before sending (§7b): bytes read of the
// files' size, conversations slimmed, bytes compressed.
export function showPrepareProgress(p){
  showLoadMeasured('progress.preparing_file', p, p.read, p.size);
}

// Sending files: bytes so far of the total, and the time left when known.
export function showSendProgress(loaded, total, eta){
  showLoadMeasured('progress.sending', { loaded, total, eta }, loaded, total);
}

// Waiting for the server to process an upload: which attempt, why the last
// one failed, a clock (core/upload-wait.js's describeWait), and, when the
// answer says, how far it has got, which moves the bar.
export function showWaitProgress(answer, elapsedMs){
  const progress = processingProgress(answer.progress);
  if(progress) loadBar().fillTo(progress.done, progress.total);
  setLoadProgressLabel(waitMessageId(answer), {
    answer, elapsedMs, attempt: answer.attempt, max_attempts: answer.max_attempts,
  });
}

// The scan: sessions done so far of the total.
export function showScanProgress(done, total){
  showLoadMeasured('progress.scanning', {}, done, total);
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
