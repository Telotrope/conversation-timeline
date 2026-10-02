// Where the page's modules hand their activity records (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4). A port: the
// modules that notice something (the status lines, the router, the request
// helper) call these functions without knowing who, if anyone, is
// listening; the activity recorder (infra/activity-recorder.js) is connected
// here when recording is on. Nothing connected means nothing is recorded.
//
// A recorder that throws must never break the page, so a failure is
// reported in the browser console and the page carries on.

let SINK = null;

// sink: { record(event), requestStarted(), requestFinished() }, or null to
// stop recording.
export function connectActivitySink(sink){
  SINK = sink;
}

function call(method, arg){
  if(!SINK) return;
  try{
    SINK[method](arg);
  } catch(e){
    console.warn(`activity recording failed (${method}): ${e && e.message}`);
  }
}

// One event built by core/activity-event.js; the recorder adds the time and
// the tab.
export function recordActivity(event){
  call('record', event);
}

// Every request the page makes calls both, so the recorder knows when the
// page is busy talking to the server.
export function noteRequestStarted(){
  call('requestStarted');
}

export function noteRequestFinished(){
  call('requestFinished');
}
