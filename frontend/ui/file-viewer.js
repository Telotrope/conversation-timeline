// The file viewer: a card in Review opens the file it names (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §4). The server
// answers GET /files/… with a short-lived address; the file's text is read
// from there and shown by its kind (ui/render/file-view.js), never running
// any code it contains, with a link beside it to download it.

import { fileView } from '../core/file-kinds.js';
import { downloadFileText, ensureAuthToken, fetchFileAddress } from '../infra/api-client.js';
import { showFileIn } from './render/file-view.js';

// Counts the files opened, so a slow answer for a file no longer wanted is
// dropped.
let OPENED = 0;
// Addresses made for this file's download and drawing, revoked on close.
let URLS = [];

const byId = (id) => document.getElementById(id);

function objectUrl(blob){
  const url = URL.createObjectURL(blob);
  URLS.push(url);
  return url;
}

function closeViewer(){
  OPENED += 1;
  byId('fileViewer').hidden = true;
  byId('fileViewerBody').replaceChildren();
  URLS.forEach((url) => URL.revokeObjectURL(url));
  URLS = [];
}

// Close, the backdrop and "Show source", set once by main.js.
export function connectFileViewer(){
  byId('fileViewerClose').addEventListener('click', closeViewer);
  byId('fileViewer').addEventListener('click', (e) => { if(e.target === byId('fileViewer')) closeViewer(); });
}

function message(text){
  const p = document.createElement('p');
  p.className = 'hint';
  p.textContent = text;
  byId('fileViewerBody').replaceChildren(p);
}

// Opens the viewer on a file: { conversationId, messageId, number }.
export async function openFileViewer({ conversationId, messageId, number }){
  closeViewer();
  const opened = OPENED;
  byId('fileViewer').hidden = false;
  byId('fileViewerTitle').textContent = 'Opening the file…';
  byId('fileViewerNote').textContent = '';
  byId('fileViewerDownload').hidden = true;
  byId('fileViewerSource').hidden = true;
  message('Fetching the file…');
  let file, text;
  try{
    const token = await ensureAuthToken(byId('devLoginSub').value.trim());
    file = await fetchFileAddress(token, conversationId, messageId, number);
    text = await downloadFileText(file.url);
  } catch(err){
    console.error(err);
    if(opened === OPENED) message(`Could not open the file: ${err.message}`);
    return;
  }
  if(opened !== OPENED) return;
  const view = fileView(file.kind);
  byId('fileViewerTitle').textContent = file.name;
  byId('fileViewerNote').textContent = view.label + (file.may_have_changed_later ? ' · this copy may have been changed later' : '');
  const download = byId('fileViewerDownload');
  download.href = objectUrl(new Blob([text], { type: view.mime }));
  download.download = file.name;
  download.hidden = false;
  const shown = showFileIn(byId('fileViewerBody'), text, file.kind, objectUrl);
  const toggle = byId('fileViewerSource');
  toggle.hidden = shown === null;
  if(shown){
    toggle.textContent = 'Show source';
    toggle.onclick = () => {
      const showing = toggle.textContent === 'Show source';
      shown.source(showing);
      toggle.textContent = showing ? 'Show the page' : 'Show source';
    };
  }
}
