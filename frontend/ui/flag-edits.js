// Your flag changes. Ticking a checkbox or pressing Approve records all three
// flags on that message as yours and saves them to the backend; flipping the
// show-automatic or show-mine switches changes what every view counts.
// Either way, every view is redrawn.

import { effectiveFlag } from '../core/flags.js';
import { state } from '../core/state.js';
import { patchFlagsToBackend } from '../infra/api-client.js';
import { refreshAllViews } from './refresh-views.js';
import { setSaveStatus } from './widgets/status-indicators.js';

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
  patchFlagsToBackend(msg, values).then(setSaveStatus);
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
