// Turns on the activity log for this page load (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4): makes the request
// tracker and the recorder, connects the recorder to the port the rest of
// the page records through (core/activity-sink.js), and hands back what the
// listeners (ui/activity-listeners.js, installed by main.js) need.
//
// Recording starts at once, so nothing done while the deployment's settings
// are read is missed, but nothing is sent until decideActivityRecording
// says recording is on; off forgets what was recorded.

import { connectActivitySink } from '../core/activity-sink.js';
import { createRequestTracker } from '../core/request-tracker.js';
import { createActivityRecorder } from '../infra/activity-recorder.js';
import { lastAuthToken, postActivityBatch } from '../infra/api-client.js';

let RECORDER = null;

// The tab shown now, '' on the load screen: read from the page once at the
// start, then kept up to date by the page as it changes (core/activity-sink.js's
// noteTabShown and noteMainShown), so a click never has to look it up (plan C18).
let ACTIVE_TAB = '';
let MAIN_SHOWN = false;

function currentTab(){
  return MAIN_SHOWN ? ACTIVE_TAB : '';
}

const warn = (message) => console.warn(message);

// Returns { record(event), leave(), now(), warn(message) } for the listeners.
export function startActivityCapture(){
  const active = document.querySelector('nav.tabs button.active');
  ACTIVE_TAB = active ? active.dataset.tab : '';
  const main = document.getElementById('mainContent');
  MAIN_SHOWN = Boolean(main) && main.style.display !== 'none';
  const now = () => Date.now();
  const tracker = createRequestTracker(now);
  RECORDER = createActivityRecorder({
    now,
    tracker,
    token: lastAuthToken,
    post: postActivityBatch,
    defer: (fn) => setTimeout(fn, 0),
    currentTab,
    warn,
  });
  const recorder = RECORDER;
  connectActivitySink({
    record: (event) => recorder.record(event),
    requestStarted: () => tracker.start(),
    requestFinished: () => tracker.finish(),
    tabShown: (name) => { ACTIVE_TAB = name; },
    mainShown: (shown) => { MAIN_SHOWN = shown; },
  });
  return {
    record: (event) => recorder.record(event),
    leave: () => recorder.leave(),
    now,
    warn,
  };
}

// on: whether this page load's activity is recorded; pageVersion: the
// version its records carry. See initLogin in ui/login-panel.js for how
// both are decided. Off also disconnects the recorder, so the rest of the
// page records nothing more.
export function decideActivityRecording(on, pageVersion){
  RECORDER.decide(on, pageVersion);
  if(!on) connectActivitySink(null);
}
