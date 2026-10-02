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

// The tab shown now, '' on the load screen.
function currentTab(){
  const main = document.getElementById('mainContent');
  if(!main || main.style.display === 'none') return '';
  const active = document.querySelector('nav.tabs button.active');
  return active ? active.dataset.tab : '';
}

const warn = (message) => console.warn(message);

// Returns { record(event), leave(), now(), warn(message) } for the listeners.
export function startActivityCapture(){
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
  });
  return {
    record: (event) => recorder.record(event),
    leave: () => recorder.leave(),
    now,
    warn,
  };
}

// on: whether this page load's activity is recorded; see initLogin in
// ui/login-panel.js for how that is decided. Off also disconnects the
// recorder, so the rest of the page records nothing more.
export function decideActivityRecording(on){
  RECORDER.decide(on);
  if(!on) connectActivitySink(null);
}
