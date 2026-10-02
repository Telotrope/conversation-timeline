// Counts the page's own requests that are still waiting for an answer, and
// remembers when the last one finished, so activity records are sent only
// when the page is quiet (plan docs/plans/2026-10-02-activity-instrumentation.md
// §4). `now` is the clock, a function returning milliseconds.

export function createRequestTracker(now){
  let inFlight = 0;
  let lastFinishedAt = null;
  return {
    start(){ inFlight += 1; },
    // Never below zero: a finish without a start (a bug elsewhere) must not
    // make the page look permanently busy or permanently quiet.
    finish(){
      inFlight = Math.max(0, inFlight - 1);
      lastFinishedAt = now();
    },
    inFlight(){ return inFlight; },
    // null until a request has finished.
    lastFinishedAt(){ return lastFinishedAt; },
  };
}
