// Draws the Files tab: every file the user has uploaded, newest first, with
// how many conversations first came in it, their details, and whether those
// are still the upload's guess (plan docs/plans/2026-10-05-screen-flow.md
// §7e). Edit details opens the Describe page for that file, through the
// handler main.js sets, since views don't import the page flow.

import { describeMedium, describeOrigin, describeParticipants } from '../../core/conversation-metadata.js';
import { formatDateTime } from '../../core/format.js';
import { state } from '../../core/state.js';
import { escapeHtml } from '../render/markup.js';

let EDIT_FILE = () => {};

export function setFileEditHandler(fn){
  EDIT_FILE = fn;
}

function countsText(u){
  const parts = [`${u.conversation_count} new conversation${u.conversation_count === 1 ? '' : 's'}`];
  if(u.already_present){
    parts.push(`${u.already_present} already present${u.gained_messages ? `, ${u.gained_messages} gained messages` : ''}`);
  }
  return parts.join('; ');
}

function row(u){
  const own = u.conversation_count > 0;
  const written = u.file_written_at ? ` · written ${formatDateTime(u.file_written_at)}` : '';
  return `<tr data-upload-id="${escapeHtml(u.upload_id)}">
    <td>${escapeHtml(u.file_name)}<span class="meta">uploaded ${formatDateTime(u.uploaded_at)}${written}</span></td>
    <td>${countsText(u)}</td>
    <td>${own ? escapeHtml(describeMedium(u.medium)) : '—'}</td>
    <td>${own ? escapeHtml(describeParticipants(u.participants)) : '—'}</td>
    <td>${own ? describeOrigin(u.details_origin) : '—'}</td>
    <td>${own ? '<button type="button" class="btn-secondary btn-small edit-file">Edit details</button>' : ''}</td>
  </tr>`;
}

// Drawn only with a timeline, which needs at least one file, so the list
// is never empty.
export function renderFiles(){
  const body = document.getElementById('filesBody');
  body.innerHTML = `<table class="files">
    <thead><tr><th>File</th><th>Conversations</th><th>Kind</th><th>Participants</th><th>Details</th><th></th></tr></thead>
    <tbody>${state.uploads.map(row).join('')}</tbody>
  </table>`;
  body.querySelectorAll('.edit-file').forEach((button) => {
    button.addEventListener('click', () => EDIT_FILE(button.closest('tr').dataset.uploadId));
  });
}
