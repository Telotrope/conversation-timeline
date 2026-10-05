// The page's progress and status lines: the load screen's message and
// progress bar with its time-remaining estimate, the notice that a previous
// session was restored, and the "Saved." / "Could not save" line.

import { formatEta } from '../../core/format.js';
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

// id null clears the line, which shows nothing and isn't recorded.
export function setLoadStatus(id, values = {}){
  const el = document.getElementById('loadStatus');
  if(id === null){
    el.textContent = '';
    return;
  }
  const entry = pageMessage(id);
  el.textContent = entry.text(values);
  el.style.color = entry.isError ? '#B0392F' : 'var(--ink-faint)';
  recordShown('loadStatus', id, entry, values);
}

// --- Load-screen progress ---
// Three states, because the load has three genuinely different kinds of
// phase: a measurable transfer, an unmeasurable wait, and finished. Nothing
// here invents a percentage for work whose size isn't known.

export function showLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  const fill = document.getElementById('loadProgressFill');
  wrap.style.display = 'block';
  fill.classList.remove('is-error', 'is-working');
  fill.style.width = '0%';
  document.getElementById('loadProgressLabel').textContent = '';
}

export function hideLoadProgress(){
  document.getElementById('loadProgress').style.display = 'none';
}

export function failLoadProgress(){
  const wrap = document.getElementById('loadProgress');
  if(wrap.style.display === 'none') return;
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
  document.getElementById('loadProgress').style.display = 'block';
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

export function showRestoredNotice(sub){
  const notice = document.getElementById('restoredNotice');
  const entry = pageMessage('restored');
  document.getElementById('restoredNoticeText').textContent = entry.text({ sub });
  notice.style.display = 'flex';
  recordShown('restoredNotice', 'restored', entry);
  document.getElementById('restoredNoticeDismiss').onclick = () => { notice.style.display = 'none'; };
}

// An error message here only marks the activity record; the line looks the
// same either way.
export function setSaveStatus(id, values = {}){
  const entry = pageMessage(id);
  const el = document.getElementById('saveStatus');
  if(el) el.textContent = entry.text(values);
  recordShown('saveStatus', id, entry, values);
}
