// Draws a message's content in Review (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4, §4c): its text
// pieces formatted from Markdown with a small numbered link after each cited
// span, and a card for each file in the place the message presented it.
// Everything from the message is escaped; a cited address becomes a link
// only when it is a web address, opened in a new tab that can't reach back
// into this page.

import { MARK, isWebAddress, withCitationMarks } from '../../core/citations.js';
import { fileView } from '../../core/file-kinds.js';
import { escapeAttribute, escapeHtml, renderMarkdownLite } from './markup.js';

function citationHtml(source){
  const address = escapeAttribute(source.address.address);
  if(isWebAddress(source.address)){
    return `<sup class="cite"><a href="${address}" target="_blank" rel="noopener noreferrer" title="${address}">${source.number}</a></sup>`;
  }
  return `<sup class="cite" title="${address}">${source.number}</sup>`;
}

// One text piece: the markers go in before the Markdown is formatted, since
// the citations' positions count the raw text.
export function textPieceHtml(piece){
  const { text, sources } = withCitationMarks(piece.text, piece.citations || []);
  const byNumber = new Map(sources.map((s) => [s.number, s]));
  return renderMarkdownLite(text).replace(MARK, (_, n) => citationHtml(byNumber.get(Number(n))));
}

// A file's card. A card whose contents are stored opens the file
// (ui/file-viewer.js); one whose contents aren't in the export says so.
export function fileCardHtml(file, conversationId, messageId){
  const view = fileView(file.kind);
  const stored = file.contents === 'stored';
  const notes = [];
  if(!stored) notes.push('not included in the export');
  if(file.may_have_changed_later) notes.push('this copy may have been changed later');
  return `<button type="button" class="file-card${stored ? '' : ' is-missing'}"${stored ? '' : ' disabled'}`
    + ` data-file-conv="${escapeAttribute(conversationId)}" data-file-msg="${escapeAttribute(messageId)}" data-file-number="${Number(file.number)}">`
    + `<span class="file-card-name">${escapeHtml(file.name)}</span>`
    + `<span class="file-card-kind">${escapeHtml(view.label)}</span>`
    + notes.map((n) => `<span class="file-card-note">${n}</span>`).join('')
    + '</button>';
}

// A message's pieces in order, then its attachments; '' for none.
export function contentHtml(pieces, attachments, conversationId, messageId){
  const parts = pieces.map((piece) => (piece.type === 'file'
    ? fileCardHtml(piece.file, conversationId, messageId)
    : textPieceHtml(piece)));
  const files = attachments.map((file) => fileCardHtml(file, conversationId, messageId));
  if(files.length) parts.push(`<div class="file-cards">${files.join('')}</div>`);
  return parts.join('');
}
