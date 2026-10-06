// Prepares a chosen file for sending, in a Web Worker (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §7b): the worker
// (workers/prepare-upload.js) slims the export and gzip-compresses it while
// the page keeps moving its bar and clock. One worker per file, ended once
// it answers.
//
// A file that isn't an export the worker can slim is sent as it is, so the
// server still says what is wrong with it, as it always has.

import { PageError } from '../core/page-error.js';

export const WORKER_URL = new URL('../workers/prepare-upload.js', import.meta.url);

// onProgress({ read, size, conversations, compressed }) about every half
// second. registerAbort(fn) receives a function that stops the worker (the
// Upload page's Stop); a stopped preparation rejects. Resolves to
// { body, slimmed: true, conversations, slimmedBytes } with body the
// compressed Blob, or { body: file, slimmed: false, reason } for a file sent
// as it is.
export function prepareUpload(file, onProgress, registerAbort = () => {}){
  return new Promise((resolve, reject) => {
    const worker = new Worker(WORKER_URL, { type: 'module' });
    const fail = (reason) => {
      worker.terminate();
      reject(new PageError(`preparing the file failed: ${reason}`, 'other'));
    };
    registerAbort(() => {
      worker.terminate();
      reject(new PageError('preparing the file was stopped', 'aborted'));
    });
    worker.addEventListener('message', ({ data }) => {
      if(data.kind === 'progress') return onProgress(data);
      worker.terminate();
      if(data.kind === 'done'){
        resolve({ body: data.blob, slimmed: true, conversations: data.conversations, slimmedBytes: data.slimmed });
      } else if(data.kind === 'not_export'){
        console.info(`sending ${file.name} as it is: ${data.reason}`);
        resolve({ body: file, slimmed: false, reason: data.reason });
      } else {
        fail(data.reason);
      }
    });
    worker.addEventListener('error', (event) => fail(event.message || 'the worker could not start'));
    worker.addEventListener('messageerror', () => fail("the worker's answer could not be read"));
    worker.postMessage({ file });
  });
}
