// Draws the "Review & flags" tab, where you check and correct flags: a paged,
// searchable, filterable table of your messages with a checkbox per flag;
// the banner explaining the current filter; and the entry points other views
// use to open it on a conversation, time span or day.

import { localDateKey } from '../../core/blocks.js';
import { extractMessageText } from '../../core/export-format.js';
import { effectiveFlag, hasUserValue, isOverridden } from '../../core/flags.js';
import { state } from '../../core/state.js';
import { switchTab } from '../navigation/tabs.js';
import { escapeHtml, renderMarkdownLite } from '../render/markup.js';

// --- Review tab ---
const PAGE_SIZE = 50;

let reviewPage = 0;

let reviewConvFilter = null;   // conversation index, or null for all conversations

let reviewRangeFilter = null;  // {start, end} in ms, or null for no time restriction

let reviewDayFilter = null;    // 'YYYY-MM-DD' (local), or null — mutually exclusive with conv/range

let reviewHighlightIds = null; // message ids to flash/scroll to after render

// What a flag checkbox and an Approve button do, supplied once by main.js.
// The review table can't import them itself: they redraw every view,
// including this one, so the two files would need each other.
let onFlagToggled = null;   // (id, type, checked) => void

let onRowApproved = null;   // (id) => void

export function setFlagEditHandlers(toggle, approve){
  onFlagToggled = toggle;
  onRowApproved = approve;
}

// Back to the first page of results, for a changed search or filter.
export function showFirstReviewPage(){
  reviewPage = 0;
  renderReviewTable();
}

function getFilteredHumanMessages(){
  const search = document.getElementById('reviewSearch').value.trim().toLowerCase();
  const filter = document.getElementById('reviewFilter').value;

  let results = state.humanMessages.filter(m=>{
    if(reviewDayFilter !== null){
      if(localDateKey(new Date(m.ts)) !== reviewDayFilter) return false;
    } else {
      if(reviewConvFilter !== null && m.conv !== reviewConvFilter) return false;
      if(reviewRangeFilter){
        const t = new Date(m.ts).getTime();
        if(t < reviewRangeFilter.start || t > reviewRangeFilter.end) return false;
      }
    }
    if(search && !m.text.toLowerCase().includes(search)) return false;
    const eCrit = effectiveFlag(m, 'critical');
    const eAngry = effectiveFlag(m, 'angry');
    const eCaps = effectiveFlag(m, 'caps');
    if(filter === 'flagged' && !(eCrit || eAngry || eCaps)) return false;
    if(filter === 'caps' && !eCaps) return false;
    if(filter === 'angry' && !eAngry) return false;
    if(filter === 'critical' && !eCrit) return false;
    if(filter === 'overridden' && !(isOverridden(m,'critical') || isOverridden(m,'angry') || isOverridden(m,'caps'))) return false;
    return true;
  });

  if(reviewDayFilter !== null){
    // Day view: grouped by conversation name, chronological within each —
    // never interleaved across conversations.
    results = results.slice().sort((a,b)=>{
      const nameA = state.conversations[a.conv].name, nameB = state.conversations[b.conv].name;
      if(nameA !== nameB) return nameA < nameB ? -1 : 1;
      return new Date(a.ts) - new Date(b.ts);
    });
  } else {
    results = results.slice().sort((a,b)=> new Date(a.ts) - new Date(b.ts));
  }
  return results;
}

// Jump into the Review tab from elsewhere on the page (a flag icon, a
// session time span, or a "chat message review" link), optionally
// restricted to one conversation and/or one time range, and optionally
// with specific rows flashed/scrolled into view once rendered.
export function jumpToReview({conv=null, rangeStart=null, rangeEnd=null, flagType='all', highlightIds=null} = {}){
  reviewConvFilter = conv;
  reviewRangeFilter = (rangeStart != null && rangeEnd != null) ? {start: rangeStart, end: rangeEnd} : null;
  reviewDayFilter = null;
  reviewHighlightIds = highlightIds;
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = flagType;
  reviewPage = 0;
  switchTab('review');
  renderReviewTable();
}

// Jump straight to a whole-day view across every conversation active that
// day (from clicking a date label on the Calendar tab, or "View entire day"
// from a narrower filter).
export function jumpToReviewDay(dateKey){
  reviewDayFilter = dateKey;
  reviewConvFilter = null;
  reviewRangeFilter = null;
  reviewHighlightIds = null;
  document.getElementById('reviewSearch').value = '';
  document.getElementById('reviewFilter').value = 'all';
  reviewPage = 0;
  switchTab('review');
  renderReviewTable();
}

function shiftReviewDay(deltaDays){
  if(reviewDayFilter === null) return;
  const d = new Date(reviewDayFilter + 'T00:00:00');
  d.setDate(d.getDate() + deltaDays);
  reviewDayFilter = localDateKey(d);
  reviewPage = 0;
  renderReviewTable();
}

function clearReviewFilters(){
  reviewConvFilter = null;
  reviewRangeFilter = null;
  reviewDayFilter = null;
  reviewHighlightIds = null;
  document.getElementById('reviewFilter').value = 'all';
  reviewPage = 0;
  renderReviewTable();
}

// Renders a flag's checkbox cell according to the current state.showAuto/state.showUser
// state (see the four-row table in effectiveFlag's comment):
//  - both on:  editable, labeled "auto"/"you"
//  - auto only: read-only, shows auto value, no label
//  - your tags only: editable, labeled "tagged"/"untagged" (stated vs not)
//  - both off: caller skips this entirely (no columns at all)
function checkboxCell(msg, type){
  const val = effectiveFlag(msg, type);
  const autoTitle = msg.auto_source === 'llm' ? 'title="automatic tag from Claude, zero-shot"' : (msg.auto_source === 'heuristic' ? 'title="automatic tag from keyword/sentiment heuristic"' : '');
  if(state.showAuto && !state.showUser){
    return `<div class="flag-checkbox">
      <input type="checkbox" disabled ${val ? 'checked' : ''}>
    </div>`;
  }
  if(!state.showAuto && state.showUser){
    const stated = hasUserValue(msg, type);
    return `<div class="flag-checkbox">
      <input type="checkbox" data-id="${msg.id}" data-type="${type}" ${val ? 'checked' : ''}>
      <span class="src">${stated ? 'tagged' : 'untagged'}</span>
    </div>`;
  }
  // both on
  const overridden = isOverridden(msg, type);
  return `<div class="flag-checkbox${overridden ? ' is-override' : ''}">
    <input type="checkbox" data-id="${msg.id}" data-type="${type}" ${val ? 'checked' : ''}>
    <span class="src" ${overridden ? '' : autoTitle}>${overridden ? 'you' : (msg.auto_source === 'llm' ? 'AI' : 'auto')}</span>
  </div>`;
}

function renderReviewFilterBanner(){
  const el = document.getElementById('reviewFilterBanner');

  if(reviewDayFilter !== null){
    el.style.display = 'flex';
    const d = new Date(reviewDayFilter + 'T00:00:00');
    const label = d.toLocaleDateString(undefined, {weekday:'long', month:'long', day:'numeric', year:'numeric'});
    el.innerHTML = `
      <span>
        <button id="prevDayBtn" class="btn-secondary" style="padding:4px 10px;">◀</button>
        Day: <strong>${label}</strong>
        <button id="nextDayBtn" class="btn-secondary" style="padding:4px 10px;">▶</button>
      </span>
      <button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`;
    document.getElementById('prevDayBtn').addEventListener('click', ()=> shiftReviewDay(-1));
    document.getElementById('nextDayBtn').addEventListener('click', ()=> shiftReviewDay(1));
    document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
    return;
  }

  if(reviewConvFilter === null && !reviewRangeFilter){
    el.style.display = 'none';
    el.innerHTML = '';
    return;
  }
  el.style.display = 'flex';
  const convName = reviewConvFilter !== null ? state.conversations[reviewConvFilter].name : null;
  let label = '';
  if(convName) label += `Conversation: <strong>${escapeHtml(convName)}</strong>`;
  let dayKeyForButton = null;
  if(reviewRangeFilter){
    const s = new Date(reviewRangeFilter.start), e = new Date(reviewRangeFilter.end);
    dayKeyForButton = localDateKey(s);
    label += `${label ? ' · ' : ''}Time span: <strong>${s.toLocaleString(undefined,{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'})} – ${e.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</strong>`;
  }

  const buttons = [];
  if(reviewRangeFilter && reviewConvFilter !== null){
    buttons.push(`<button id="viewEntireConvBtn" class="btn-secondary">View entire conversation</button>`);
    buttons.push(`<button id="viewEntireDayBtn" class="btn-secondary">View entire day</button>`);
  }
  buttons.push(`<button id="clearReviewFilter" class="btn-secondary">Clear filter</button>`);

  el.innerHTML = `<span>${label}</span><span style="display:flex; gap:8px;">${buttons.join('')}</span>`;
  document.getElementById('clearReviewFilter').addEventListener('click', clearReviewFilters);
  const viewConvBtn = document.getElementById('viewEntireConvBtn');
  if(viewConvBtn) viewConvBtn.addEventListener('click', ()=>{
    reviewRangeFilter = null;
    reviewPage = 0;
    renderReviewTable();
  });
  const viewDayBtn = document.getElementById('viewEntireDayBtn');
  if(viewDayBtn) viewDayBtn.addEventListener('click', ()=> jumpToReviewDay(dayKeyForButton));
}

function renderReplyRow(msg){
  if(!state.showReplies) return '';
  const convMsgs = state.rawData[msg.conv] && state.rawData[msg.conv].chat_messages;
  if(!convMsgs) return '';
  const next = convMsgs[msg.rawIndex + 1];
  if(!next || next.sender !== 'assistant') return '';
  const replyText = extractMessageText(next);
  if(!replyText) return '';
  const colCount = (state.showAuto || state.showUser) ? 6 : 3;
  return `<tr class="claude-reply-row">
    <td colspan="${colCount}"><span class="who-label">Claude</span>${renderMarkdownLite(replyText)}</td>
  </tr>`;
}

export function renderReviewTable(){
  renderReviewFilterBanner();
  const filtered = getFilteredHumanMessages();
  const totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  reviewPage = Math.min(reviewPage, totalPages - 1);
  const pageItems = filtered.slice(reviewPage * PAGE_SIZE, (reviewPage+1) * PAGE_SIZE);

  document.getElementById('reviewCount').textContent = `${filtered.length} message${filtered.length===1?'':'s'}`;

  const showFlagColumns = state.showAuto || state.showUser;

  const rows = pageItems.map(m => {
    const conv = state.conversations[m.conv];
    const dt = new Date(m.ts);
    const flagCells = showFlagColumns ? `
      <td class="flag-cell">${checkboxCell(m, 'caps')}</td>
      <td class="flag-cell">${checkboxCell(m, 'angry')}</td>
      <td class="flag-cell">${checkboxCell(m, 'critical')}</td>
      ${state.showUser ? `<td class="flag-cell"><button class="approve-btn" data-id="${m.id}">Approve</button></td>` : ''}
    ` : '';
    const mainRow = `<tr data-msg-id="${m.id}">
      <td class="when">${dt.toLocaleDateString(undefined,{month:'short',day:'numeric',year:'numeric'})}<br>${dt.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</td>
      <td class="conv-name">${escapeHtml(conv.name)}</td>
      <td class="msg-text">${m.text ? renderMarkdownLite(m.text) : '<em style="color:var(--ink-faint);">(no text — attachment only)</em>'}</td>
      ${flagCells}
    </tr>`;
    return mainRow + renderReplyRow(m);
  }).join('');

  const approveHeader = state.showUser ? '<th></th>' : '';
  const flagHeaders = showFlagColumns ? `<th>All caps</th><th>Angry</th><th>Critical</th>${approveHeader}` : '';

  document.getElementById('reviewTable').innerHTML = `
    <table class="review">
      <thead><tr>
        <th>When</th><th>Conversation</th><th>Message</th>
        ${flagHeaders}
      </tr></thead>
      <tbody>${rows}</tbody>
    </table>`;

  if(showFlagColumns && state.showUser){
    document.querySelectorAll('.flag-checkbox input:not([disabled])').forEach(cb=>{
      cb.addEventListener('change', (e)=>{
        const id = e.target.dataset.id;
        const type = e.target.dataset.type;
        onFlagToggled(id, type, e.target.checked);
      });
    });
    document.querySelectorAll('.approve-btn').forEach(btn=>{
      btn.addEventListener('click', ()=> onRowApproved(btn.dataset.id));
    });
  }

  document.getElementById('pagination').innerHTML = `
    <button id="prevPage" ${reviewPage===0?'disabled':''}>Previous</button>
    <span>Page ${reviewPage+1} of ${totalPages}</span>
    <button id="nextPage" ${reviewPage>=totalPages-1?'disabled':''}>Next</button>`;
  document.getElementById('prevPage').addEventListener('click', ()=>{ reviewPage--; renderReviewTable(); });
  document.getElementById('nextPage').addEventListener('click', ()=>{ reviewPage++; renderReviewTable(); });

  if(reviewHighlightIds && reviewHighlightIds.length){
    const idsToHighlight = reviewHighlightIds;
    requestAnimationFrame(()=>{
      let firstRow = null;
      idsToHighlight.forEach(id=>{
        const row = document.querySelector(`tr[data-msg-id="${id}"]`);
        if(row){
          row.classList.add('row-highlight');
          if(!firstRow) firstRow = row;
        }
      });
      if(firstRow && typeof firstRow.scrollIntoView === 'function') firstRow.scrollIntoView({behavior:'smooth', block:'center'});
    });
    reviewHighlightIds = null;
  }
}
