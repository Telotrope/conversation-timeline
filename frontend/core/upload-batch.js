// Uploading several files at once (plan docs/plans/2026-10-05-screen-flow.md
// §6): every file starts together, their sent bytes add up to one combined
// progress, one file failing leaves the rest running, and Stop ends them
// all. No page access: the work of one file is handed in.

// Starts the batch. `files` are { name, size } (a browser File is one);
// `uploadOne(file, ctx)` does one file's work and resolves to its upload
// id, using `ctx`:
//   ctx.sent(loaded)        bytes of this file sent so far
//   ctx.sendingDone()       its bytes have all been sent
//   ctx.registerAbort(fn)   fn cancels what it is doing now
//   ctx.sleep(ms)           a pause that ends early, rejecting, on Stop
// `on` holds the batch's callbacks: progress(loaded, total) for the bytes
// sent across every file, and failed(index, error) as each file fails.
//
// Returns { done, stop }. `done` resolves, once every file has finished,
// to { processed: [{ index, uploadId }], failed: [{ index, error }],
// stopped: [index] }. `stop()` cancels every file still at work; those
// count as stopped, not failed.
export function startBatch(files, uploadOne, on){
  let stopped = false;
  const aborts = new Set();
  const loaded = files.map(() => 0);
  const total = files.reduce((sum, f) => sum + f.size, 0);
  const pauses = stoppablePauses(() => stopped);

  const runOne = async (file, index) => {
    const ctx = {
      sent: (n) => { loaded[index] = n; on.progress(loaded.reduce((a, b) => a + b, 0), total); },
      sendingDone: () => { loaded[index] = file.size; on.progress(loaded.reduce((a, b) => a + b, 0), total); },
      // A file that reaches its send after Stop is cancelled at once.
      registerAbort: (fn) => { if(stopped) fn(); else aborts.add(fn); },
      sleep: pauses.sleep,
    };
    try{
      return { index, uploadId: await uploadOne(file, ctx) };
    } catch(error){
      if(stopped) return { index, stopped: true };
      on.failed(index, error);
      return { index, error };
    }
  };

  const stop = () => {
    stopped = true;
    aborts.forEach((fn) => fn());
    pauses.wakeAll();
  };

  return { done: Promise.all(files.map(runOne)).then(sortResults), stop };
}

// Pauses that end early, rejecting, when Stop is pressed (`isStopped()`
// true at the start, or wakeAll() while waiting).
function stoppablePauses(isStopped){
  const wakers = new Set();
  const sleep = (ms) => new Promise((resolve, reject) => {
    if(isStopped()){ reject(new StoppedError()); return; }
    const waker = () => { clearTimeout(timer); reject(new StoppedError()); };
    const timer = setTimeout(() => { wakers.delete(waker); resolve(); }, ms);
    wakers.add(waker);
  });
  return { sleep, wakeAll: () => wakers.forEach((wake) => wake()) };
}

function sortResults(results){
  return {
    processed: results.filter((r) => r.uploadId !== undefined).map(({ index, uploadId }) => ({ index, uploadId })),
    failed: results.filter((r) => r.error !== undefined).map(({ index, error }) => ({ index, error })),
    stopped: results.filter((r) => r.stopped).map((r) => r.index),
  };
}

// What a pause rejects with when Stop is pressed.
export class StoppedError extends Error {
  constructor(){
    super('stopped');
    this.name = 'StoppedError';
  }
}
