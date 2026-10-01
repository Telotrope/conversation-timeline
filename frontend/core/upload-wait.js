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
// Resolves when processing is done; rejects with the server's reason if it
// failed, or after GIVE_UP_AFTER_MS.
export async function waitForProcessing({ fetchStatus, sleep, now }){
  const started = now();
  for(;;){
    const answer = await fetchStatus();
    if(answer.status === 'ready') return;
    if(answer.status === 'failed'){
      throw new Error(`the server couldn't process the file: ${answer.reason}`);
    }
    if(answer.status !== 'processing'){
      throw new Error(`the server answered with an unknown upload status: ${JSON.stringify(answer.status)}`);
    }
    const elapsed = now() - started;
    if(elapsed >= GIVE_UP_AFTER_MS){
      throw new Error('the server is still processing the file after 10 minutes; try reloading later');
    }
    await sleep(elapsed < FAST_PERIOD_MS ? FAST_INTERVAL_MS : SLOW_INTERVAL_MS);
  }
}
