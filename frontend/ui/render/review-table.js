// Draws Review's table for the page of rows on screen (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §4d): each of
// your messages with a checkbox per flag and, with "Show Claude's replies"
// on, the reply that follows it; and a note in its own row wherever a
// replaced branch was pruned. Nothing here asks the server or listens for
// clicks; ui/views/review.js does both.

import { effectiveFlag, isOverridden, isReviewed } from '../../core/flags.js';
import { formatClock } from '../../core/format.js';
import { noteWording } from '../../core/review-rows.js';
import { state } from '../../core/state.js';
import { escapeAttribute, escapeHtml } from './markup.js';
import { contentHtml } from './reply-markup.js';

// Says once per row whether you have reviewed it, wherever your tags are
// shown. A review covers all three flags, so a per-box label would repeat it.
function reviewStatus(msg){
  if(!state.showUser) return '';
  const reviewed = isReviewed(msg);
  return `<span class="review-status${reviewed ? ' is-reviewed' : ''}">${reviewed ? 'Reviewed' : 'Not reviewed'}</span>`;
}

// Renders a flag's checkbox cell according to the current state.showAuto/state.showUser
// state (see the four-row table in effectiveFlag's comment):
//  - both on:  editable; the row says once whether it is reviewed
//  - auto only: read-only, shows auto value, no label
//  - your tags only: editable, no label; the row says once whether it has
//    been reviewed (see reviewStatus), since reviewing covers all three flags
//  - both off: caller skips this entirely (no columns at all)
function checkboxCell(msg, type){
  const val = effectiveFlag(msg, type);
  const id = escapeAttribute(msg.id);
  if(state.showAuto && !state.showUser){
    return `<div class="flag-checkbox">
      <input type="checkbox" disabled ${val ? 'checked' : ''}>
    </div>`;
  }
  if(!state.showAuto && state.showUser){
    return `<div class="flag-checkbox">
      <input type="checkbox" data-id="${id}" data-type="${type}" ${val ? 'checked' : ''}>
    </div>`;
  }
  // Both on: the row's label says whether it is reviewed. Hovering a box
  // whose value is automatic says where that value came from.
  const autoTitle = msg.auto_source === 'heuristic' ? 'title="automatic tag from keyword/sentiment heuristic"' : '';
  return `<div class="flag-checkbox">
    <input type="checkbox" data-id="${id}" data-type="${type}" ${val ? 'checked' : ''} ${isOverridden(msg, type) ? '' : autoTitle}>
  </div>`;
}

function columnCount(){
  if(!(state.showAuto || state.showUser)) return 3;
  return state.showUser ? 7 : 6;
}

function whenCell(at){
  if(!at) return '<td class="when">time unknown</td>';
  const dt = new Date(at);
  return `<td class="when">${dt.toLocaleDateString(undefined,{month:'short',day:'numeric',year:'numeric'})}<br>${dt.toLocaleTimeString(undefined,{hour:'numeric',minute:'2-digit'})}</td>`;
}

function replyRow(row){
  if(!state.showReplies || !row.reply) return '';
  const content = contentHtml(row.reply.pieces, [], row.conversation_id, row.reply.message_id);
  if(!content) return '';
  return `<tr class="claude-reply-row" data-reply-id="${escapeAttribute(row.reply.message_id)}">
    <td colspan="${columnCount()}"><span class="who-label">Claude</span>${content}</td>
  </tr>`;
}

function messageRow(row, msg, conversationName){
  const showFlagColumns = state.showAuto || state.showUser;
  const id = escapeAttribute(msg.id);
  const flagCells = showFlagColumns ? `
      <td class="flag-cell">${checkboxCell(msg, 'caps')}</td>
      <td class="flag-cell">${checkboxCell(msg, 'angry')}</td>
      <td class="flag-cell">${checkboxCell(msg, 'critical')}</td>
      ${state.showUser ? `<td class="flag-cell"><button class="approve-btn" data-id="${id}">Approve</button>${reviewStatus(msg)}</td>` : ''}
    ` : '';
  const content = contentHtml(row.pieces, row.attachments, row.conversation_id, row.message_id);
  return `<tr data-msg-id="${id}">
      ${whenCell(row.at)}
      <td class="conv-name">${escapeHtml(conversationName)}</td>
      <td class="msg-text">${content || '<em class="no-text">(no text)</em>'}</td>
      ${flagCells}
    </tr>` + replyRow(row);
}

// conversationIndex(id): the conversation's index in state.conversations,
// or -1, for the link to a branch kept as its own conversation.
function noteRow(note, conversationIndex){
  const { lead, link, rest } = noteWording(note, formatClock);
  let linked = '';
  if(link !== null){
    const idx = conversationIndex(note.kept_as);
    linked = idx >= 0 ? `<a href="#conversations/${idx}">${escapeHtml(link)}</a>` : escapeHtml(link);
  }
  return `<tr class="note-row"><td colspan="${columnCount()}">${escapeHtml(lead)}${linked}${escapeHtml(rest)}</td></tr>`;
}

// The table for `rows`, each message row with its message (`messages`, by
// id; core/review-rows.js's messageOf). conversationName(id) names a
// conversation; conversationIndex(id) finds it.
export function reviewTableHtml(rows, messages, { conversationName, conversationIndex }){
  const showFlagColumns = state.showAuto || state.showUser;
  const body = rows.map((row) => (row.kind === 'note'
    ? noteRow(row, conversationIndex)
    : messageRow(row, messages.get(row.message_id), conversationName(row.conversation_id)))).join('');
  const approveHeader = state.showUser ? '<th></th>' : '';
  const flagHeaders = showFlagColumns ? `<th>All caps</th><th>Angry</th><th>Critical</th>${approveHeader}` : '';
  return `
    <table class="review">
      <thead><tr>
        <th>When</th><th>Conversation</th><th>Message</th>
        ${flagHeaders}
      </tr></thead>
      <tbody>${body}</tbody>
    </table>`;
}
