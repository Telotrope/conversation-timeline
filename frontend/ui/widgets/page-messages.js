// Every message the page's status lines show, by a fixed identifier (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, C17). The activity
// log records the identifier, never the wording; the wording lives only
// here, so the two can't drift apart. Callers name a message by its
// identifier and hand in its live values; the status setters
// (status-indicators.js) look the wording up.
//
// Each entry: `text(values)`, the wording (null for a message that changes
// no wording), `isError`, and `record`, the live values the activity log
// keeps: numbers, and an error's kind (`error_kind`, from
// core/page-error.js). Values not listed (the server's error message, a
// name) appear on the page only.

import { describeWait } from '../../core/upload-wait.js';

const failure = ['status', 'error_kind'];
const wait = { text: ({ answer, elapsedMs }) => describeWait(answer, elapsedMs), isError: false, record: ['attempt', 'max_attempts'] };

export const PAGE_MESSAGES = Object.freeze({
  // The load screen's status line.
  'load.choose_file': { text: () => 'Choose a conversations.json file first.', isError: true },
  'load.signing_in': { text: () => 'Logging in…', isError: false },
  'load.sending': { text: () => 'Sending your file…', isError: false },
  'load.processing': { text: () => 'Processing on the server…', isError: false },
  'load.scanning': { text: () => 'Scanning your messages for flags…', isError: false },
  'load.no_conversations': {
    text: () => 'That file parsed, but contained no conversations — is it the right export?', isError: true,
  },
  'load.failed': {
    text: ({ detail, hint }) => `Could not load that file through the backend — ${detail}${hint}`,
    isError: true, record: failure,
  },

  // The label under the load screen's progress bar.
  'progress.signing_in': { text: () => 'Signing in…', isError: false },
  'progress.reading_file': { text: () => 'Reading the file…', isError: false },
  'progress.processing': { text: () => 'Processing on the server…', isError: false },
  'progress.preparing': { text: () => 'Preparing the timeline…', isError: false },
  'progress.failed': { text: null, isError: true }, // the bar turns red; its label stays
  // While the server processes an upload; see core/upload-wait.js's
  // waitMessageId, which picks one of these for each answer.
  'wait.waiting': wait,
  'wait.processing': wait,
  'wait.error': wait,
  'wait.will_retry': wait,
  'wait.retrying': wait,

  // The review tab's save line.
  'save.saved': { text: () => 'Saved.', isError: false },
  'save.not_logged_in': { text: () => 'Not saved to the server — log in first.', isError: true, record: failure },
  'save.no_server_id': {
    text: () => "Could not save — couldn't find this message's server-side id.", isError: true, record: failure,
  },
  'save.server_error': { text: ({ detail }) => `Could not save to the server: ${detail}`, isError: true, record: failure },
  'save.stale_page': {
    text: () => "Could not save: this page's data is out of date. Reload the page.", isError: true, record: failure,
  },
  'flags.loaded': {
    text: ({ count }) => `Loaded ${count} of your confirmed flag${count === 1 ? '' : 's'} from the server.`,
    isError: false, record: ['count'],
  },
  'export.no_data': { text: () => 'No conversation data loaded to annotate.', isError: true },
  'export.downloaded': {
    text: () => 'Downloaded conversations-with-flags.json — load this file directly next time.', isError: false,
  },

  // The notice that a previous session was picked up again. `sub` is the
  // name it was loaded under (with a real sign-in, the email address).
  restored: {
    text: ({ sub }) => `Picked up where you left off — the export you last loaded as "${sub}". Use "Load a different file…" to start fresh.`,
    isError: false,
  },

  // The sign-in line, with a deployment's sign-in.
  'signIn.signed_in': { text: ({ who }) => `Signed in as ${who}.`, isError: false },
  'signIn.signed_out': { text: () => 'Sign in to upload your export.', isError: false },
  'signIn.failed': {
    text: ({ name, detail }) => `Signing in to "${name}" isn't working: ${detail}`, isError: true, record: failure,
  },
  'signIn.start_failed': { text: ({ detail }) => `Could not start signing in: ${detail}`, isError: true, record: failure },
});

// The entry for an identifier; an unknown one is a mistake in the page's
// code, so it throws naming it.
export function pageMessage(id){
  const entry = Object.prototype.hasOwnProperty.call(PAGE_MESSAGES, id) ? PAGE_MESSAGES[id] : undefined;
  if(!entry) throw new Error(`no page message named ${JSON.stringify(id)}`);
  return entry;
}

// The live values the activity log keeps for a message (see `record`).
export function recordedValues(entry, values){
  if(!entry.record || !values) return undefined;
  const kept = {};
  for(const key of entry.record) if(values[key] !== undefined && values[key] !== null) kept[key] = values[key];
  return kept;
}
