// Your flag changes. Ticking a checkbox or pressing Approve records all three
// flags on that message as yours and saves them to the backend; flipping the
// show-automatic or show-mine switches changes what every view counts.
// Either way, every view is redrawn.

import { effectiveFlag } from '../core/flags.js';
import { state } from '../core/state.js';
import { SaveOutcome, patchFlagsToBackend } from '../infra/api-client.js';
import { refreshAllViews } from './refresh-views.js';
import { setSaveStatus } from './widgets/status-indicators.js';

// The save indicator's wording for each outcome of a save.
function saveMessage({ outcome, detail }){
  switch(outcome){
    case SaveOutcome.SAVED: return 'Saved.';
    case SaveOutcome.NOT_LOGGED_IN: return 'Not saved to the server — log in first.';
    case SaveOutcome.NO_SERVER_ID: return "Could not save — couldn't find this message's server-side id.";
    case SaveOutcome.SERVER_ERROR: return 'Could not save to the server: ' + detail;
    case SaveOutcome.STALE_PAGE: return "Could not save: this page's data is out of date. Reload the page.";
  }
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
  patchFlagsToBackend(msg, values)
    .then((result) => setSaveStatus(saveMessage(result), result.outcome !== SaveOutcome.SAVED));
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
