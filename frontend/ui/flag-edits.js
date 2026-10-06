// Your flag changes. Ticking a checkbox or pressing Approve records all three
// flags on that message as yours and saves them to the backend; flipping the
// show-automatic or show-mine switches changes what every view counts.
// Either way, every view is redrawn.
//
// A save answers with the message's session counted again (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §6): that session
// replaces the page's copy, the Calendar, the Conversations tab and the
// analysis are drawn again from it, and Review asks for the same page again.

import { toBlock } from '../core/blocks.js';
import { effectiveFlag } from '../core/flags.js';
import { state } from '../core/state.js';
import { SaveOutcome, patchFlagsToBackend } from '../infra/api-client.js';
import { refreshAllViews } from './refresh-views.js';
import { markReviewStale, refreshReviewPage, renderReviewTable, reviewMessage } from './views/review.js';
import { setSaveStatus } from './widgets/status-indicators.js';

// The save indicator's message (ui/widgets/page-messages.js) for each
// outcome of a save, and the error kind the activity log records for it.
const SAVE_MESSAGES = {
  [SaveOutcome.SAVED]: ['save.saved', undefined],
  [SaveOutcome.NOT_LOGGED_IN]: ['save.not_logged_in', 'not_logged_in'],
  [SaveOutcome.NO_SERVER_ID]: ['save.no_server_id', 'no_server_id'],
  [SaveOutcome.SERVER_ERROR]: ['save.server_error', undefined],
  [SaveOutcome.STALE_PAGE]: ['save.stale_page', 'stale_page'],
};

function showSaveResult({ outcome, detail, status, errorKind }){
  const [id, kind] = SAVE_MESSAGES[outcome];
  setSaveStatus(id, { detail, status, error_kind: errorKind || kind });
}

// The saved message's session, counted again by the server, in place of
// the page's copy; and the data version the save raised.
function takeSavedSession(reply){
  const s = reply.session;
  const i = state.blocks.findIndex((b) => b.number === s.number && state.conversations[b.conv].id === s.conversation_id);
  if(i >= 0) state.blocks[i] = toBlock(s, state.blocks[i].conv);
  else console.warn(`a saved flag's session (${s.conversation_id} #${s.number}) isn't on the page; reload to see its counts`);
  state.dataVersion = reply.data_version;
  refreshAllViews();
  refreshReviewPage();
}

// Clicking any single checkbox, or the Approve button, promotes ALL THREE
// flags on that message to explicit user values at once — using the
// just-changed value for `changedType` (if any) and the message's current
// effective value for the other two. This is what "approving a row" means:
// one click reviews the whole message, not just the box you touched.
export function setRowOverrides(id, changedType, changedValue){
  const msg = reviewMessage(id);
  if(!msg) return;
  const values = {
    caps: changedType === 'caps' ? changedValue : effectiveFlag(msg, 'caps'),
    angry: changedType === 'angry' ? changedValue : effectiveFlag(msg, 'angry'),
    critical: changedType === 'critical' ? changedValue : effectiveFlag(msg, 'critical'),
  };
  state.overrides[id] = values;
  renderReviewTable();
  patchFlagsToBackend(msg, values).then((result) => {
    showSaveResult(result);
    if(result.outcome === SaveOutcome.SAVED) takeSavedSession(result.reply);
  });
}

export function approveRow(id){
  setRowOverrides(id, null, null);
}

export function onVisibilityToggleChanged(){
  state.showAuto = document.getElementById('toggleShowAuto').checked;
  state.showUser = document.getElementById('toggleShowUser').checked;
  refreshAllViews();
  markReviewStale();
}
