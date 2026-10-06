// Decides whether one of the messages on Review's page counts as flagged,
// combining the server's automatic flags, your corrections and the show
// switches. Sessions come from the server already counted under every view
// (core/session-counts.js); the same rules are the server's `FlagView`
// (plan docs/plans/2026-10-06-load-only-what-the-page-shows.md §5b), so
// "flagged" means the same on the page and in the counts.

import { state } from './state.js';

export function hasUserValue(msg, type){
  const o = state.overrides[msg.id];
  return !!(o && typeof o[type] === 'boolean');
}

export function effectiveFlag(msg, type){
  if(state.showAuto && !state.showUser){
    // Auto-only view: overrides are ignored entirely (not deleted, just not shown).
    return msg['default_' + type];
  }
  if(!state.showAuto && state.showUser){
    // Your-tags-only view: auto is not used as a fallback; no stated
    // preference just means "nothing", not "whatever auto thinks".
    return hasUserValue(msg, type) ? state.overrides[msg.id][type] : false;
  }
  if(!state.showAuto && !state.showUser) return false;
  // Both on: normal behavior — your override wins if you've stated one.
  return hasUserValue(msg, type) ? state.overrides[msg.id][type] : msg['default_' + type];
}

// "Overridden" in the ON/ON sense (used for the "auto"/"you" label).
export function isOverridden(msg, type){
  return hasUserValue(msg, type);
}

// Whether you have reviewed this message's row. Reviewing is per row: a
// checkbox or Approve records all three flags at once.
export function isReviewed(msg){
  const o = state.overrides[msg.id];
  return !!o && ['caps', 'angry', 'critical'].some((t) => typeof o[t] === 'boolean');
}

// Whether any of the three flags is in effect for this message.
export function isFlagged(msg){
  return effectiveFlag(msg, 'critical') || effectiveFlag(msg, 'angry') || effectiveFlag(msg, 'caps');
}
