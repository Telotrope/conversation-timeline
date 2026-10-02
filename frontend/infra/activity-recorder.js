// Keeps the page's activity records in memory and sends them to the
// backend's POST /activity in batches, only when the page is quiet (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, "When events are
// sent"):
//
// - only when none of the page's own requests has been in flight for 3 s;
// - at most once a minute, unless 500 records are waiting;
// - when the page is hidden or closed (`leave`), with fetch's keepalive so
//   the request outlives the page;
// - never on a timer: a send is only ever considered when something is
//   recorded, so a page left idle sends nothing;
// - only once there is a sign-in token; until then records wait.
//
// Recording a record is one push onto a list; turning records into JSON
// happens only when sending. If a send fails, the records are kept (at most
// 2,000, oldest dropped first), the failure is reported with console.warn,
// and the number dropped goes with the next batch that gets through.
// Nothing here throws into the page.
//
// Everything that touches the outside world is handed in, so tests can use
// fakes: the clock, the request tracker (core/request-tracker.js), the
// token, the sender and the deferral.

export const QUIET_MS = 3000;
export const MIN_INTERVAL_MS = 60_000;
export const EARLY_SEND_COUNT = 500;
export const MAX_KEPT = 2000;
export const BATCH_EVENTS = 200;          // the route's limit per request
export const MAX_EVENT_BYTES = 4096;      // the route's limit per event
// Browsers allow about 64 KiB of keepalive request bodies in flight at once;
// what doesn't fit when the page closes is lost.
export const KEEPALIVE_BUDGET_BYTES = 60_000;

// options:
//   now()          -> milliseconds since the epoch
//   tracker        -> { inFlight(), lastFinishedAt() }
//   token()        -> the current sign-in token, or null
//   post(body, { token, keepalive }) -> Promise<{ ok, status }>; body is the
//                     batch's JSON text
//   defer(fn)      -> runs fn after the current event has been handled
//   currentTab()   -> the tab shown now ('' if none); called on every
//                     record, so it should only read a remembered value
//   warn(message)  -> reports a failed send (console.warn in the page)
export function createActivityRecorder({ now, tracker, token, post, defer, currentTab, warn }){
  // 'undecided' (recording, not yet sending: the deployment's settings
  // aren't read yet), 'on', or 'off'.
  let mode = 'undecided';
  let kept = [];
  let dropped = 0;
  let lastSendAt = null;
  let sending = false;
  let scheduled = false;
  // The page's version, added to every record as it is sent (not as it is
  // recorded: it is known only once the deployment's settings are read, and
  // adding it while serializing makes no extra object per click).
  let versionTail = ',"page_version":"unknown"}';

  function keep(events){
    kept = events.concat(kept);
    trim();
  }

  function trim(){
    const excess = kept.length - MAX_KEPT;
    if(excess > 0){
      kept.splice(0, excess);
      dropped += excess;
    }
  }

  function due(){
    if(mode !== 'on' || sending || kept.length === 0 || !token()) return false;
    if(tracker.inFlight() > 0) return false;
    const t = now();
    const finished = tracker.lastFinishedAt();
    if(finished !== null && t - finished < QUIET_MS) return false;
    if(lastSendAt !== null && t - lastSendAt < MIN_INTERVAL_MS && kept.length < EARLY_SEND_COUNT) return false;
    return true;
  }

  // Takes records off the front of the list for one request body: at most
  // BATCH_EVENTS of them, and at most maxBytes of JSON. A record too large
  // for the route is dropped and counted. carryDropped: whether this batch
  // reports the dropped count (only one of several batches in flight at once
  // may, or the count would be cleared twice). Returns null when nothing is
  // left.
  function takeBatch(maxBytes, carryDropped){
    const parts = [];
    const taken = [];
    let size = 40;
    while(kept.length && parts.length < BATCH_EVENTS){
      const json = JSON.stringify(kept[0]).slice(0, -1) + versionTail;
      if(json.length > MAX_EVENT_BYTES){
        kept.shift();
        dropped += 1;
        warn(`activity record dropped: ${json.length} bytes is over the ${MAX_EVENT_BYTES}-byte limit`);
        continue;
      }
      if(size + json.length + 1 > maxBytes) break;
      size += json.length + 1;
      parts.push(json);
      taken.push(kept.shift());
    }
    if(!taken.length) return null;
    const droppedNow = carryDropped ? dropped : 0;
    const tail = droppedNow > 0 ? `,"dropped":${droppedNow}` : '';
    return { events: taken, droppedNow, body: `{"events":[${parts.join(',')}]${tail}}` };
  }

  // Settles one batch's answer: on success the dropped count it carried is
  // cleared; a 400 means the route will never accept it, so it is dropped
  // and counted; any other failure keeps it for the next send.
  function settle(batch, answer){
    if(answer.ok){
      dropped -= batch.droppedNow;
      return true;
    }
    const status = answer.status === null ? `no answer: ${answer.error}` : `status ${answer.status}`;
    if(answer.status === 400){
      dropped += batch.events.length;
      warn(`activity records refused (${status}); ${batch.events.length} dropped`);
    } else {
      keep(batch.events);
      warn(`activity records not sent (${status}); kept for the next send`);
    }
    return false;
  }

  function send(batch, auth, keepalive){
    return Promise.resolve()
      .then(() => post(batch.body, { token: auth, keepalive }))
      .then((answer) => answer, (e) => ({ ok: false, status: null, error: e && e.message }));
  }

  async function sendNow(){
    scheduled = false;
    if(!due()) return;
    sending = true;
    lastSendAt = now();
    const auth = token();
    try{
      for(let batch = takeBatch(Infinity, true); batch; batch = takeBatch(Infinity, true)){
        if(!settle(batch, await send(batch, auth, false))) break;
      }
    } finally {
      sending = false;
    }
  }

  return {
    // Adds one record, stamped with the time (unless it brings its own, as
    // a request does with its start) and the tab shown now. The record is
    // kept as given, stamped in place rather than copied (plan C18): it
    // belongs to the recorder from here on.
    record(event){
      if(mode === 'off') return;
      if(event.t === undefined) event.t = now();
      event.tab = currentTab();
      kept.push(event);
      trim();
      if(!scheduled && due()){
        scheduled = true;
        defer(sendNow);
      }
    },

    // Whether recording is on, once the deployment's settings are known,
    // and the page's version (a string; see core/deploy-config.js), which
    // every record is sent with. Off forgets everything recorded so far and
    // records nothing more.
    decide(on, pageVersion = 'unknown'){
      mode = on ? 'on' : 'off';
      versionTail = `,"page_version":${JSON.stringify(String(pageVersion))}}`;
      if(!on){
        kept = [];
        dropped = 0;
      }
    },

    // The page is being hidden or closed: send what fits now, ignoring the
    // quiet-moment and once-a-minute rules, with keepalive.
    leave(){
      if(mode !== 'on' || kept.length === 0) return;
      const auth = token();
      if(!auth) return;
      lastSendAt = now();
      let budget = KEEPALIVE_BUDGET_BYTES;
      for(let batch = takeBatch(budget, true); batch; batch = takeBatch(budget, false)){
        budget -= batch.body.length;
        const sent = batch;
        send(sent, auth, true).then((answer) => settle(sent, answer));
      }
    },

    // For tests and diagnostics: how many records wait, and how many have
    // been dropped since the last batch that got through.
    pending(){ return kept.length; },
    droppedCount(){ return dropped; },
  };
}
