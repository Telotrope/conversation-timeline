// Your flag changes. Ticking a checkbox or pressing Approve records all three
// flags on that message as yours and saves them to the backend; flipping the
// show-automatic or show-mine switches changes what every view counts.
// Either way, every view is redrawn.

import { effectiveFlag } from '../core/flags.js';
import { state } from '../core/state.js';
import { SaveOutcome, patchFlagsToBackend } from '../infra/api-client.js';
import { refreshAllViews } from './refresh-views.js';
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

// Clicking any single checkbox, or the Approve button, promotes ALL THREE
// flags on that message to explicit user values at once — using the
// just-changed value for `changedType` (if any) and the message's current
// effective value for the other two. This is what "approving a row" means:
// one click reviews the whole message, not just the box you touched.
export function setRowOverrides(id, changedType, changedValue){
  const msg = state.humanById.get(id);
  if(!msg) return;
  const values = {
    caps: changedType === 'caps' ? changedValue : effectiveFlag(msg, 'caps'),
    angry: changedType === 'angry' ? changedValue : effectiveFlag(msg, 'angry'),
    critical: changedType === 'critical' ? changedValue : effectiveFlag(msg, 'critical'),
  };
  state.overrides[id] = values;
  patchFlagsToBackend(msg, values).then(showSaveResult);
  refreshAllViews();
}

export function approveRow(id){
  setRowOverrides(id, null, null);
}

export function onVisibilityToggleChanged(){
  state.showAuto = document.getElementById('toggleShowAuto').checked;
  state.showUser = document.getElementById('toggleShowUser').checked;
  refreshAllViews();
}
