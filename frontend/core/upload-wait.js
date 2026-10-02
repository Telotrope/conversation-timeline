// Waits for the backend to finish processing an upload, asking
// GET /uploads/{id} until the answer is no longer "processing" (migration
// plan §V2e, E4).
//
// On AWS, processing starts only once the file has landed in S3, in a
// separate function, so the page has to ask. Locally the upload request
// processes the file before it answers, so the first answer is already
// "ready" and nothing waits.
//
// Asks once a second for the first ten seconds, then every five seconds,
// and gives up after ten minutes. Everything that touches the network or
// the clock is passed in, so tests can run it without either.

export const FAST_INTERVAL_MS = 1000;
export const FAST_PERIOD_MS = 10 * 1000;
export const SLOW_INTERVAL_MS = 5000;
export const GIVE_UP_AFTER_MS = 10 * 60 * 1000;

// fetchStatus: async () => the route's JSON ({ status, reason? }); throws
//   if the request fails, and that error is passed on unchanged.
// sleep: async (ms) => resolves after ms.
// now: () => the current time in milliseconds.
// onAnswer (optional): (answer) => called with every answer, so the page
//   can show the attempt and last error while it waits.
// Resolves when processing is done; rejects with the server's reason if it
// failed, or after GIVE_UP_AFTER_MS.
export async function waitForProcessing({ fetchStatus, sleep, now, onAnswer = () => {} }){
  const started = now();
  let last = null;
  for(;;){
    const answer = await fetchStatus();
    onAnswer(answer);
    last = answer;
    if(answer.status === 'ready') return;
    if(answer.status === 'failed'){
      throw new Error(`the server couldn't process the file: ${answer.reason}`);
    }
    if(answer.status !== 'processing'){
      throw new Error(`the server answered with an unknown upload status: ${JSON.stringify(answer.status)}`);
    }
    const elapsed = now() - started;
    if(elapsed >= GIVE_UP_AFTER_MS){
      // Not "still processing": after this long the server may have given
      // up without saying so. Report only what it last said.
      throw new Error(
        `No answer from the server after 10 minutes. Its last status was: ${describeWait(last, elapsed)}. `
        + 'Reload later to check again.');
    }
    await sleep(elapsed < FAST_PERIOD_MS ? FAST_INTERVAL_MS : SLOW_INTERVAL_MS);
  }
}

// "1:34" for 94 000 ms: minutes and zero-padded seconds.
export function formatElapsed(ms){
  const total = Math.floor(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

// The line shown under the bar while the server works on an upload (plan
// 2026-10-02-upload-processing-failures.md §3). `answer` is the latest
// "processing" answer from GET /uploads/{id}: on AWS it may carry the
// attempt number, the number of attempts AWS makes, and the last attempt's
// error; before the first attempt, and always locally, it carries none.
export function describeWait(answer, elapsedMs){
  const clock = formatElapsed(elapsedMs);
  const { attempt, max_attempts: max, last_error: error } = answer;
  if(error && attempt && attempt > 1){
    return `The server hit an error (${error}) and is trying again automatically: attempt ${attempt} of ${max}. `
      + `AWS waits 1–2 minutes between attempts. — ${clock}`;
  }
  if(error && attempt){
    return `The server hit an error (${error}) on attempt ${attempt} of ${max} and will try again automatically `
      + `in 1–2 minutes. — ${clock}`;
  }
  if(error) return `The server hit an error (${error}). — ${clock}`;
  if(attempt) return `Processing on the server — ${clock}`;
  return `Waiting for the server to start — ${clock}`;
}
