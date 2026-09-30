// Draws the Conversations tab: a searchable list of conversations and the
// transcript of the one you open, with links from its sessions and flags
// into the review tab.

import { fmtClock, fmtDayHeading, fmtDuration } from '../../core/format.js';
import { state } from '../../core/state.js';
import { rememberLocation } from '../navigation/location.js';
import { escapeHtml } from '../render/markup.js';
import { jumpToReview } from './review.js';

// --- Conversation list & detail ---
export function renderConvList(filter=''){
  const f = filter.trim().toLowerCase();
  const items = state.conversations
    .map((c, idx) => ({...c, idx}))
    .filter(c => c.name.toLowerCase().includes(f));

  document.getElementById('convItems').innerHTML = items.map(c => {
    const convBlocks = state.blocks.filter(b => b.conv === c.idx);
    const totalSec = convBlocks.reduce((a,b)=>a+b.duration_sec, 0);
    const dayCount = convBlocks.length;
    const hasCrit = convBlocks.some(b => b.criticalItems.length);
    const hasAngry = convBlocks.some(b => b.angryItems.length);
    const hasCaps = convBlocks.some(b => b.capsItems.length);
    let icons = '';
    if(hasCrit) icons += '<span class="flag-icon critical">⚑</span> ';
    if(hasAngry) icons += '<span class="flag-icon angry">!</span> ';
    if(hasCaps) icons += '<span class="flag-icon caps">A</span> ';
    return `<div class="conv-item" data-idx="${c.idx}">
      ${icons}${escapeHtml(c.name)}
      <span class="meta">${c.total_messages} messages · ${dayCount} ${dayCount===1?'day':'days'} · ${fmtDuration(totalSec)} active</span>
    </div>`;
  }).join('');

  document.querySelectorAll('.conv-item').forEach(el=>{
    el.addEventListener('click', ()=> selectConversation(parseInt(el.dataset.idx,10)));
  });
}

export function selectConversation(idx){
  state.selectedConversation = idx;
  rememberLocation();
  document.querySelectorAll('.conv-item').forEach(el=>{
    el.classList.toggle('selected', parseInt(el.dataset.idx,10) === idx);
  });
  const conv = state.conversations[idx];
  const convBlocks = state.blocks.filter(b => b.conv === idx).sort((a,b)=> a.date.localeCompare(b.date));
  const totalSec = convBlocks.reduce((a,b)=>a+b.duration_sec, 0);

  const critCount = convBlocks.reduce((a,b)=> a + b.criticalItems.length, 0);
  const angryCount = convBlocks.reduce((a,b)=> a + b.angryItems.length, 0);
  const capsCount = convBlocks.reduce((a,b)=> a + b.capsItems.length, 0);

  const rows = convBlocks.map((b, bi) => {
    const flags = [];
    if(b.criticalItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="critical" class="flag-icon critical" style="cursor:pointer;" title="${b.criticalItems.length} critical — click to review">⚑</span>`);
    if(b.angryItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="angry" class="flag-icon angry" style="cursor:pointer;" title="${b.angryItems.length} angry — click to review">!</span>`);
    if(b.capsItems.length) flags.push(`<span data-block-idx="${b._idx}" data-flag-type="caps" class="flag-icon caps" style="cursor:pointer;" title="${b.capsItems.length} ALL-CAPS — click to review">A</span>`);
    return `
    <tr class="session-row" data-block-idx="${b._idx}" style="cursor:pointer;" title="Click to review these messages">
      <td>${fmtDayHeading(b.date)}</td>
      <td>${fmtClock(b.start)} – ${fmtClock(b.end)}</td>
      <td class="dur">${fmtDuration(b.duration_sec)}</td>
      <td>${b.count}</td>
      <td>${flags.join(' ')}</td>
    </tr>`;
  }).join('');

  const notes = [];
  if(critCount) notes.push(`<span class="flag-icon critical">⚑</span> ${critCount} critical`);
  if(angryCount) notes.push(`<span class="flag-icon angry">!</span> ${angryCount} angry`);
  if(capsCount) notes.push(`<span class="flag-icon caps">A</span> ${capsCount} ALL-CAPS`);
  const critNote = notes.length
    ? `<div class="summary" style="display:flex; gap:14px; align-items:center;">${notes.join('')} — click a flag or a row below to review those messages.</div>`
    : '';

  document.getElementById('convDetail').innerHTML = `
    <h3>${escapeHtml(conv.name)}</h3>
    <div class="summary">${conv.total_messages} messages total · active across ${convBlocks.length} ${convBlocks.length===1?'day':'days'} · ${fmtDuration(totalSec)} of combined active time</div>
    ${critNote}
    <table class="sessions">
      <thead><tr><th>Day</th><th>Time span</th><th>Duration</th><th>Messages</th><th>Flags</th></tr></thead>
      <tbody>${rows}</tbody>
    </table>
    <a href="#" id="chatReviewLink" style="display:inline-block; margin-top:18px; font-size:0.85rem; color:var(--accent);">Chat message review →</a>`;

  // Flag icon click: jump to Review showing the whole session for context,
  // with the flagged message(s) highlighted — not filtered to just that type.
  document.querySelectorAll('#convDetail .flag-icon[data-flag-type]').forEach(el=>{
    el.addEventListener('click', (e)=>{
      e.stopPropagation();
      const b = state.blocks[parseInt(el.dataset.blockIdx, 10)];
      const type = el.dataset.flagType;
      const items = type === 'critical' ? b.criticalItems : type === 'angry' ? b.angryItems : b.capsItems;
      jumpToReview({
        conv: b.conv,
        rangeStart: new Date(b.start).getTime(),
        rangeEnd: new Date(b.end).getTime(),
        flagType: 'all',
        highlightIds: items.map(m=>m.id),
      });
    });
  });

  // Row click (not on a flag icon): jump to Review showing all messages in that session
  document.querySelectorAll('#convDetail .session-row').forEach(el=>{
    el.addEventListener('click', ()=>{
      const b = state.blocks[parseInt(el.dataset.blockIdx, 10)];
      jumpToReview({
        conv: b.conv,
        rangeStart: new Date(b.start).getTime(),
        rangeEnd: new Date(b.end).getTime(),
        flagType: 'all',
        highlightIds: b.allHuman.map(m=>m.id),
      });
    });
  });

  // "Chat message review" link: jump to Review showing the whole conversation, unrestricted by time
  document.getElementById('chatReviewLink').addEventListener('click', (e)=>{
    e.preventDefault();
    jumpToReview({ conv: idx, flagType: 'all' });
  });
}
