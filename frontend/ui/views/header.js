// Fills in the line under the page title, for example "412 conversations,
// 18,300 messages, 2025-01-04 to 2026-09-20."

import { state } from '../../core/state.js';

// --- Header stats ---
export function renderSubtitle(){
  const totalMsgs = state.conversations.reduce((a,c)=>a+c.total_messages,0);
  const dates = state.blocks.map(b=>b.date).sort();
  document.getElementById('subtitle').textContent =
    `${state.conversations.length} conversations, ${totalMsgs.toLocaleString()} messages, ${dates[0]} to ${dates[dates.length-1]}.`;
}
