// Decides whether a message counts as flagged, combining the backend's
// automatic flags, your corrections and the show switches, and attaches the
// same answer to each session block so every view shows the same counts.

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

// Attach flags to whichever block each flagged human message falls within
// (same conversation, timestamp inside [start, end]; if it lands in a gap
// that got split out as idle time, attach to the nearest block instead).
export function attachFlags(){
  state.blocks.forEach(b=>{
    b.criticalItems = [];
    b.angryItems = [];
    b.capsItems = [];
    b.allHuman = [];
  });
  state.humanMessages.forEach(msg=>{
    const t = new Date(msg.ts).getTime();
    const convBlocks = state.blocks.filter(b => b.conv === msg.conv);
    if(convBlocks.length === 0) return;
    let best = null, bestDist = Infinity;
    convBlocks.forEach(b=>{
      const s = new Date(b.start).getTime(), e = new Date(b.end).getTime();
      const dist = t < s ? s - t : (t > e ? t - e : 0);
      if(dist < bestDist){ bestDist = dist; best = b; }
    });
    // Currently unreachable: the early return above leaves at least one
    // session, and any session is nearer than Infinity. Kept as a backstop
    // if the search above changes.
    if(!best) return;
    best.allHuman.push(msg);
    if(effectiveFlag(msg, 'critical')) best.criticalItems.push(msg);
    if(effectiveFlag(msg, 'angry')) best.angryItems.push(msg);
    if(effectiveFlag(msg, 'caps')) best.capsItems.push(msg);
  });
  state.blocks.forEach(b => b.allHuman.sort((a,c)=> new Date(a.ts) - new Date(c.ts)));
}

// Whether you have reviewed this message's row. Reviewing is per row: a
// checkbox or Approve records all three flags at once.
export function isReviewed(msg){
  const o = state.overrides[msg.id];
  return !!o && ['caps', 'angry', 'critical'].some((t) => typeof o[t] === 'boolean');
}

// Whether this message has flag values under the current show switches, and
// so belongs in a rate's denominator. With automatic tags shown, every
// message has a value; with only yours shown, only reviewed rows do; with
// neither, none do. Counting the rest as "not flagged" would report
// unreviewed messages as clean.
export function countsTowardRates(msg){
  if(state.showAuto) return true;
  if(state.showUser) return isReviewed(msg);
  return false;
}

// Whether any of the three flags is in effect for this message.
export function isFlagged(msg){
  return effectiveFlag(msg, 'critical') || effectiveFlag(msg, 'angry') || effectiveFlag(msg, 'caps');
}
