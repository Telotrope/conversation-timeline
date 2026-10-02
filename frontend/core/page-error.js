// A failure the page knows the kind of. The message is for you to read on
// the page; the kind (and HTTP status, when there is one) is what the
// activity log records, never the message: a server's or browser's error
// message can repeat parts of the request (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, C17).

// Every kind of failure the activity log can name. 'other' catches the
// rest; a kind earns its own name only when it is told apart somewhere.
export const ERROR_KINDS = Object.freeze([
  'network',            // no answer at all (fetch's TypeError)
  'aborted',            // the request was cancelled
  'server_error',       // the server answered with a failure status
  'processing_failed',  // the server said it couldn't process the upload
  'unknown_status',     // the server answered with an upload status the page doesn't know
  'timed_out',          // the page stopped waiting for the server
  'stale_page',         // a flag save refused: the page's data is out of date
  'not_logged_in',
  'no_server_id',       // a message with no server-side id to save under
  'other',
]);

export class PageError extends Error {
  // kind: one of ERROR_KINDS. status: the HTTP status, or null.
  constructor(message, kind, status = null){
    // The name stays 'Error', so the message reads as it did before this
    // class existed (tests/upload-wait.test.js matches it whole).
    super(message);
    this.kind = kind;
    this.status = status;
  }
}

// The kind of any thrown value: a PageError's own kind; fetch's TypeError
// is a request that got no answer; an AbortError a cancelled one;
// anything else 'other'.
export function errorKindOf(error){
  if(error instanceof PageError) return ERROR_KINDS.includes(error.kind) ? error.kind : 'other';
  if(error instanceof TypeError) return 'network';
  if(error && error.name === 'AbortError') return 'aborted';
  return 'other';
}

// The HTTP status a thrown value carries, or null.
export function errorStatusOf(error){
  return error instanceof PageError && Number.isInteger(error.status) ? error.status : null;
}
