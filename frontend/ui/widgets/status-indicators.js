// The page's progress and status lines: the load screen's message and
// progress bar with its time-remaining estimate, the notice that a previous
// session was restored, and the "Saved." / "Could not save" line.

import { formatEta } from '../../core/format.js';
import { shownEvent } from '../../core/activity-event.js';
import { recordActivity } from '../../core/activity-sink.js';

// Each message shown here is also recorded for the activity log (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, `shown`). Clearing a
// line (an empty message) shows nothing and isn't recorded.
function recordShown(where, text, isError){
  if(text) recordActivity(shownEvent(where, text, isError));
}

export function setLoadStatus(msg, isError){
  const el = document.getElementById('loadStatus');
  el.textContent = msg;
  el.style.color = isError ? '#B0392F' : 'var(--ink-faint)';
  recordShown('loadStatus', msg, isError);
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
  recordShown('loadProgress', document.getElementById('loadProgressLabel').textContent || 'failed', true);
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
export function setLoadProgressIndeterminate(label){
  document.getElementById('loadProgress').style.display = 'block';
  const fill = document.getElementById('loadProgressFill');
  fill.classList.add('is-working');
  fill.style.width = '100%';
  document.getElementById('loadProgressLabel').textContent = label;
  recordShown('loadProgress', label, false);
}

// Before a transfer whose progress is measured: a plain bar again.
export function setLoadProgressMeasured(){
  document.getElementById('loadProgressFill').classList.remove('is-working');
}

// Replaces the label only, keeping the bar as it is.
export function setLoadProgressLabel(label){
  document.getElementById('loadProgressLabel').textContent = label;
  recordShown('loadProgress', label, false);
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
  document.getElementById('restoredNoticeText').textContent =
    `Picked up where you left off — the export you last loaded as "${sub}". Use "Load a different file…" to start fresh.`;
  notice.style.display = 'flex';
  // A fixed wording, not the notice's text: with a real sign-in the name in
  // it is your email address.
  recordShown('restoredNotice', 'Picked up where you left off', false);
  document.getElementById('restoredNoticeDismiss').onclick = () => { notice.style.display = 'none'; };
}

// isError only marks the activity record; the line looks the same either way.
export function setSaveStatus(msg, isError = false){
  const el = document.getElementById('saveStatus');
  if(el) el.textContent = msg;
  recordShown('saveStatus', msg, isError);
}
