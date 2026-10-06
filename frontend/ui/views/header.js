// Fills in the line under the page title, for example "412 conversations,
// 18,300 messages, 2025-01-04 to 2026-09-20."

import { localDateKey } from '../../core/blocks.js';
import { state } from '../../core/state.js';

// --- Header stats ---
// The first and last local days come from the earliest start and the
// latest end of the sessions.
export function renderSubtitle(){
  const totalMsgs = state.conversations.reduce((a,c)=>a+c.total_messages,0);
  let first = Infinity, last = -Infinity;
  for(const b of state.blocks){
    first = Math.min(first, Date.parse(b.start));
    last = Math.max(last, Date.parse(b.end));
  }
  const span = state.blocks.length
    ? `, ${localDateKey(new Date(first))} to ${localDateKey(new Date(last))}`
    : '';
  document.getElementById('subtitle').textContent =
    `${state.conversations.length} conversations, ${totalMsgs.toLocaleString()} messages${span}.`;
}
