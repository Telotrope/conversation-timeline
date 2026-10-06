// Draws the Conversations tab: a searchable list of conversations and, for
// the one you open, its details, its sessions with their flags, and the
// files presented in it, with links from each into the review tab.
//
// Drawn from the conversations' records and the sessions' stored counts
// (plan docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §6); the
// list of files is asked of the server when a conversation opens (§4).

import { describeMedium, describeOrigin, describeParticipants } from '../../core/conversation-metadata.js';
import { formatClock, formatDateTime, formatDayHeading, formatDuration } from '../../core/format.js';
import { localDaysTouched } from '../../core/blocks.js';
import { fileView } from '../../core/file-kinds.js';
import { readAllParts, joinParts } from '../../core/parts.js';
import { viewCounts, viewName } from '../../core/session-counts.js';
import { state } from '../../core/state.js';
import { ensureAuthToken, fetchConversationFilesPart } from '../../infra/api-client.js';
import { rememberLocation } from '../navigation/location.js';
import { escapeAttribute, escapeHtml } from '../render/markup.js';
import { createProgressBar } from '../widgets/status-indicators.js';
import { jumpToReview } from './review.js';

let EDIT_CONVERSATION = () => {};

// Edit details on the open conversation opens the Describe page for it,
// through the handler main.js sets (views don't import the page flow).
export function setConversationEditHandler(fn){
  EDIT_CONVERSATION = fn;
}

function conversationLink(id){
  const idx = state.conversations.findIndex((c) => c.id === id);
  if(idx < 0) return 'a conversation no longer here';
  return `<a href="#conversations/${idx}">${escapeHtml(state.conversations[idx].name)}</a>`;
}

// A conversation kept from an earlier branch of another, and the branches
// kept from this one (§4d): both show the link.
function branchLine(r){
  const parts = [];
  if(r.branch_of) parts.push(`An earlier branch of ${conversationLink(r.branch_of)}, kept as its own conversation.`);
  if(r.branches && r.branches.length){
    parts.push(`Earlier branches kept as their own conversations: ${r.branches.map(conversationLink).join(', ')}.`);
  }
  return parts.length ? `<div class="conv-branches">${parts.join(' ')}</div>` : '';
}

// The open conversation's details, from its record (plan
// docs/plans/2026-10-05-screen-flow.md §7e): kind, who took part, start and
// end, and whether these are still guessed. Empty without a record.
function detailsLine(conv){
  const r = state.records.get(conv.id);
  if(!r) return '';
  return `<div class="conv-details">
    <span>${escapeHtml(describeMedium(r.medium))} · ${escapeHtml(describeParticipants(r.participants))}</span>
    <span>${formatDateTime(r.span.start)} – ${formatDateTime(r.span.end)}</span>
    <span>${describeOrigin(r.details_origin)} · from ${escapeHtml(r.source.file_name)}</span>
    <button type="button" id="editConversationBtn" class="btn-secondary btn-small">Edit details</button>
  </div>${branchLine(r)}`;
}

// --- Conversation list & detail ---
// How many distinct local days these sessions touch. Several sessions on
// one day count once; a session crossing midnight counts both days.
function daysActive(blocks){
  return new Set(blocks.flatMap(b => localDaysTouched(b).map(p => p.date))).size;
}

// Each flag's count across these sessions under the current view.
function flagTotals(blocks, view){
  const totals = { critical: 0, angry: 0, caps: 0 };
  for(const b of blocks){
    const counts = viewCounts(b.counts, view);
    for(const type of Object.keys(totals)) totals[type] += counts[type];
  }
  return totals;
}

// Every conversation's sessions, by its index.
function blocksByConversation(){
  const byConv = new Map();
  state.blocks.forEach((b, i) => {
    if(!byConv.has(b.conv)) byConv.set(b.conv, []);
    byConv.get(b.conv).push({ b, i });
  });
  return byConv;
}

function listItemHtml(c, idx, entries, view){
  const blocks = entries.map((e) => e.b);
  const totalSec = blocks.reduce((a,b)=>a+b.duration_sec, 0);
  const dayCount = daysActive(blocks);
  const totals = flagTotals(blocks, view);
  let icons = '';
  if(totals.critical) icons += '<span class="flag-icon critical">⚑</span> ';
  if(totals.angry) icons += '<span class="flag-icon angry">!</span> ';
  if(totals.caps) icons += '<span class="flag-icon caps">A</span> ';
  return `<div class="conv-item" data-idx="${idx}">
      ${icons}${escapeHtml(c.name)}
      <span class="meta">${c.total_messages} messages · ${dayCount} ${dayCount===1?'day':'days'} · ${formatDuration(totalSec)} active</span>
    </div>`;
}

// The list's drawing, one step per conversation; yields { done, total }.
export function* convListSteps(filter = ''){
  const f = filter.trim().toLowerCase();
  const view = viewName(state.showAuto, state.showUser);
  const byConv = blocksByConversation();
  const html = [];
  const total = state.conversations.length;
  for(const [idx, c] of state.conversations.entries()){
    if(c.name.toLowerCase().includes(f)) html.push(listItemHtml(c, idx, byConv.get(idx) || [], view));
    yield { done: idx + 1, total };
  }
  document.getElementById('convItems').innerHTML = html.join('');
}

export function renderConvList(filter = ''){
  const steps = convListSteps(filter);
  while(!steps.next().done);
}

function sessionRowHtml({ b, i }, view){
  const counts = viewCounts(b.counts, view);
  const flags = [];
  if(counts.critical) flags.push(`<span data-block-idx="${i}" data-flag-type="critical" class="flag-icon critical clickable" title="${counts.critical} critical — click to review">⚑</span>`);
  if(counts.angry) flags.push(`<span data-block-idx="${i}" data-flag-type="angry" class="flag-icon angry clickable" title="${counts.angry} angry — click to review">!</span>`);
  if(counts.caps) flags.push(`<span data-block-idx="${i}" data-flag-type="caps" class="flag-icon caps clickable" title="${counts.caps} ALL-CAPS — click to review">A</span>`);
  return `
    <tr class="session-row clickable" data-block-idx="${i}" title="Click to review these messages">
      <td>${formatDayHeading(b.date)}</td>
      <td>${formatClock(b.start)} – ${formatClock(b.end)}</td>
      <td class="dur">${formatDuration(b.duration_sec)}</td>
      <td>${b.count}</td>
      <td>${flags.join(' ')}</td>
    </tr>`;
}

export function selectConversation(idx){
  state.selectedConversation = idx;
  rememberLocation();
  document.querySelectorAll('.conv-item').forEach(el=>{
    el.classList.toggle('selected', parseInt(el.dataset.idx,10) === idx);
  });
  const view = viewName(state.showAuto, state.showUser);
  const conv = state.conversations[idx];
  const entries = (blocksByConversation().get(idx) || []).sort((a, c) => new Date(a.b.start) - new Date(c.b.start));
  const blocks = entries.map((e) => e.b);
  const dayCount = daysActive(blocks);
  const totalSec = blocks.reduce((a,b)=>a+b.duration_sec, 0);
  const totals = flagTotals(blocks, view);

  const notes = [];
  if(totals.critical) notes.push(`<span class="flag-icon critical">⚑</span> ${totals.critical} critical`);
  if(totals.angry) notes.push(`<span class="flag-icon angry">!</span> ${totals.angry} angry`);
  if(totals.caps) notes.push(`<span class="flag-icon caps">A</span> ${totals.caps} ALL-CAPS`);
  const critNote = notes.length
    ? `<div class="summary with-flags">${notes.join('')} — click a flag or a row below to review those messages.</div>`
    : '';

  document.getElementById('convDetail').innerHTML = `
    <h3>${escapeHtml(conv.name)}</h3>
    ${detailsLine(conv)}
    <div class="summary">${conv.total_messages} messages total · active across ${dayCount} ${dayCount===1?'day':'days'} · ${formatDuration(totalSec)} of combined active time</div>
    ${critNote}
    <h4 class="table-title">Sessions</h4>
    <table class="sessions">
      <thead><tr><th>Day</th><th>Time span</th><th>Duration</th><th>Messages</th><th>Flags</th></tr></thead>
      <tbody>${entries.map((e) => sessionRowHtml(e, view)).join('')}</tbody>
    </table>
    <a href="#" id="chatReviewLink" class="review-link">Chat message review →</a>
    <h4 class="table-title">Files</h4>
    <div id="convFiles" class="conv-files">
      <div class="progress-wrap"><div class="progress-track"><div class="progress-fill" id="convFilesFill"></div></div>
      <div class="progress-label" id="convFilesLabel"></div></div>
    </div>`;

  const edit = document.getElementById('editConversationBtn');
  if(edit) edit.addEventListener('click', () => EDIT_CONVERSATION(conv.id));

  // "Chat message review" link: jump to Review showing the whole conversation, unrestricted by time
  document.getElementById('chatReviewLink').addEventListener('click', (e)=>{
    e.preventDefault();
    jumpToReview({ conv: idx, flagType: 'all' });
  });
  showFiles(idx);
}

// --- The open conversation's files (§4) ---

// Counts the requests for files, so an answer for a conversation no longer
// open is dropped.
let FILES_REQUEST = 0;
// Each conversation's list of files once shown, by id: a flag save redraws
// the open conversation, and its files haven't changed.
const FILES_SHOWN = new Map();

// Forgets the lists of files, for a timeline loaded again.
export function forgetConversationFiles(){
  FILES_SHOWN.clear();
}

async function showFiles(idx){
  const id = state.conversations[idx].id;
  if(FILES_SHOWN.has(id)){
    document.getElementById('convFiles').innerHTML = FILES_SHOWN.get(id);
    return;
  }
  const request = ++FILES_REQUEST;
  const current = () => request === FILES_REQUEST && state.selectedConversation === idx;
  const bar = createProgressBar({
    nodes: () => ({ fill: document.getElementById('convFilesFill'), label: document.getElementById('convFilesLabel') }),
  });
  bar.working('progress.finding_files', { done: 0, total: 0 });
  let files;
  try{
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
    const { parts } = await readAllParts((cursor) => fetchConversationFilesPart(token, id, cursor), {
      onPart: (part) => { if(current()) bar.measured('progress.finding_files', {}, part.sessions_done, part.sessions_total); },
      onRestart: () => { if(current()) bar.working('progress.data_changed'); },
    });
    files = joinParts(parts, 'files');
  } catch(err){
    console.error(err);
    bar.stop();
    if(current()) bar.failed('progress.request_failed', { detail: err.message });
    return;
  }
  bar.stop();
  FILES_SHOWN.set(id, files.length
    ? `<ul class="file-index">${files.map(fileItemHtml).join('')}</ul>`
    : '<p class="hint">No files were presented or attached in this conversation.</p>');
  if(current()) document.getElementById('convFiles').innerHTML = FILES_SHOWN.get(id);
}

function fileItemHtml(f){
  const view = fileView(f.kind);
  const who = f.sender === 'human' ? 'you' : 'Claude';
  const when = f.at ? formatDateTime(f.at) : 'time unknown';
  const stored = f.contents === 'stored' ? '' : ' · not included in the export';
  return `<li><a href="#" class="file-index-link" data-message-id="${escapeAttribute(f.message_id)}" data-at="${escapeAttribute(f.at || '')}" data-sender="${escapeAttribute(f.sender)}">${escapeHtml(f.name)}</a>
    <span class="meta">${escapeHtml(view.label)} · from ${who}, ${when}${stored}</span></li>`;
}

// Opens Review where a file appeared: the session holding its message,
// with Claude's replies shown when Claude presented it, and the message
// marked.
function reviewFile(link){
  const idx = state.selectedConversation;
  const at = link.dataset.at ? new Date(link.dataset.at).getTime() : null;
  const session = at === null ? null : state.blocks.find((b) =>
    b.conv === idx && new Date(b.start).getTime() <= at && at <= new Date(b.end).getTime());
  jumpToReview({
    conv: idx,
    rangeStart: session ? new Date(session.start).getTime() : null,
    rangeEnd: session ? new Date(session.end).getTime() : null,
    flagType: 'all',
    highlightIds: [link.dataset.messageId],
    showReplies: link.dataset.sender !== 'human',
  });
}

// One click handler for the list and one for the open conversation, set
// once by main.js.
export function connectConversations(){
  document.getElementById('convItems').addEventListener('click', (e) => {
    const item = e.target.closest('.conv-item');
    if(item) selectConversation(parseInt(item.dataset.idx, 10));
  });
  document.getElementById('convDetail').addEventListener('click', (e) => {
    const fileLink = e.target.closest('.file-index-link');
    if(fileLink){
      e.preventDefault();
      return reviewFile(fileLink);
    }
    const icon = e.target.closest('.flag-icon[data-flag-type]');
    const row = e.target.closest('.session-row');
    const target = icon || row;
    if(!target) return;
    const b = state.blocks[parseInt(target.dataset.blockIdx, 10)];
    jumpToReview({
      conv: b.conv,
      rangeStart: new Date(b.start).getTime(),
      rangeEnd: new Date(b.end).getTime(),
      flagType: icon ? icon.dataset.flagType : 'all',
    });
  });
}
