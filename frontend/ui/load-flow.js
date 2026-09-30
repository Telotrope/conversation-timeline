// Getting data onto the screen: upload the file you pick, optionally run the
// backend's flag detection page by page with progress, download the processed
// export, hand it to core/export-format.js, and draw every view. On page load
// it also reopens your last session if the backend still has it, then opens
// the view named in the web address.

import { buildBlocks } from '../core/blocks.js';
import { parseUploadedConversations } from '../core/export-format.js';
import { attachFlags } from '../core/flags.js';
import { formatBytes } from '../core/format.js';
import { state } from '../core/state.js';
import { API_BASE, clearAuthToken, describeFailure, ensureAuthToken, putWithProgress, readBodyWithProgress } from '../infra/api-client.js';
import { applyLocationHash } from './router.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList } from './views/conversations.js';
import { renderSubtitle } from './views/header.js';
import { renderReviewTable } from './views/review.js';
import { failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadStatus, setSaveStatus, showLoadProgress, showRestoredNotice } from './widgets/status-indicators.js';

// Applies an already-downloaded export: parses it, replaces the page's
// state, and shows the timeline. Returns false if the export held no
// conversations, leaving the caller to report that however suits it.
//
// Shared by the upload path and the restore-on-load path below, so a
// restored session goes through exactly the same rendering as a fresh
// upload rather than a parallel copy that can drift.
function applyExportText(text){
  const parsed = parseUploadedConversations(text);
  if(parsed.conversations.length === 0) return false;

  state.conversations = parsed.conversations;
  state.messages = parsed.messages;
  state.humanMessages = parsed.humanMessages;
  state.humanById = new Map(state.humanMessages.map(m => [m.id, m]));
  state.blocks = buildBlocks();
  state.rawData = parsed.rawData;

  // Your confirmed flags come from whatever the server's export embedded
  // (overrides you PATCHed to the backend earlier -- see
  // patchFlagsToBackend). There's no other recovery mechanism; see the
  // migration plan's V2a.
  state.overrides = { ...parsed.embeddedOverrides };
  const embeddedCount = Object.keys(parsed.embeddedOverrides).length;

  attachFlags();
  if(embeddedCount > 0){
    setSaveStatus(`Loaded ${embeddedCount} of your confirmed flag${embeddedCount===1?'':'s'} from the server.`);
  }

  document.getElementById('loadScreen').style.display = 'none';
  document.getElementById('mainContent').style.display = '';

  renderSubtitle();
  renderCalendar();
  renderConvList();
  renderReviewTable();
  return true;
}

// Picks up the last session on load, so a reload or a Back press past the
// first entry doesn't cost you the whole upload again. The backend still
// holds the processed export; all this needs is the name it was uploaded
// under.
//
// Announced rather than silent: a page that quietly opens with old data
// leaves you unsure whether you're looking at this file or the last one.
// Everything about it is best-effort -- no remembered name, a server that
// was restarted (its storage is in-memory), or anything else unexpected
// just means the normal load screen, which is the correct fallback and not
// an error worth shouting about.
export async function tryRestoreSession(){
  let sub = null;
  try{ sub = localStorage.getItem('timeline_dev_sub'); } catch(e){ return; }
  if(!sub) return;

  document.getElementById('devLoginSub').value = sub;
  try{
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
    const exportRes = await fetch(`${API_BASE}/export`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!exportRes.ok) return;
    const { export_url } = await exportRes.json();
    const downloadRes = await fetch(`${API_BASE}${export_url}`);
    if(!downloadRes.ok) return;
    if(!applyExportText(await downloadRes.text())) return;

    showRestoredNotice(sub);
    applyLocationHash();
  } catch(e){
    // The server being gone or unreachable is the ordinary case here, not a
    // fault: it just means there is nothing to restore. Logged rather than
    // swallowed so a genuinely surprising failure is still visible.
    console.info('No previous session restored:', e.message);
    clearAuthToken();
  }
}

// Runs the backend's non-generative detection pass, one page of
// conversations at a time. The server could do the whole thing in one
// request, but then there would be nothing to report: paging is what makes
// the progress bar show real, earned progress rather than a spinner.
async function runDetectionPass(token){
  const fill = document.getElementById('loadProgressFill');
  const label = document.getElementById('loadProgressLabel');
  let offset = 0;
  let detected = 0;
  for(;;){
    const res = await fetch(`${API_BASE}/detect`, {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify({ offset, limit: 5 }),
    });
    if(!res.ok) throw new Error(await describeFailure('scanning your messages', res));
    const body = await res.json();
    detected += body.messages_detected;

    const total = body.total_conversations;
    const done = body.next_offset === null || body.next_offset === undefined;
    const covered = done ? total : body.next_offset;
    const pct = total ? Math.round((covered / total) * 100) : 100;
    fill.style.width = pct + '%';
    label.textContent =
      `Scanning your messages — ${covered} of ${total} conversation${total === 1 ? '' : 's'} (${pct}%)`;

    if(done) return detected;
    offset = body.next_offset;
  }
}

export async function handleLoadClick(){
  const convInput = document.getElementById('loadConvFile');
  const convFile = convInput.files[0];
  const runDetection = document.getElementById('autoDetectCheckbox').checked;

  if(!convFile){
    setLoadStatus('Choose a conversations.json file first.', true);
    hideLoadProgress();
    return;
  }

  try{
    showLoadProgress();
    setLoadStatus('Logging in…');
    setLoadProgressIndeterminate('Signing in…');
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());

    setLoadStatus('Sending your file…');
    setLoadProgressIndeterminate('Reading the file…');
    const rawText = await convFile.text();
    const createRes = await fetch(`${API_BASE}/uploads`, {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!createRes.ok) throw new Error(await describeFailure('starting the upload', createRes));
    const { upload_url } = await createRes.json();

    const uploadFill = document.getElementById('loadProgressFill');
    const uploadLabel = document.getElementById('loadProgressLabel');
    const uploadEta = makeRateEstimator(3000);
    const putRes = await putWithProgress(`${API_BASE}${upload_url}`, rawText, (loaded, total) => {
      const pct = Math.round((loaded / total) * 100);
      uploadFill.style.width = pct + '%';
      const eta = uploadEta(loaded, total);
      uploadLabel.textContent =
        `Sending your file — ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
    });
    if(!putRes.ok){
      throw new Error(`uploading the file failed (${putRes.status})${putRes.text ? ': ' + putRes.text : ''}`);
    }

    // The bytes being sent is not the end of the wait: the local-dev PUT
    // handler parses, dedups and stores the upload before it answers, and
    // none of that is observable from here. Saying so beats a full bar that
    // looks stuck. No polling is needed either -- by the time the PUT
    // resolves the work is done. Real S3-triggered processing is
    // asynchronous; that gap isn't solved here, see the migration plan's V2a.
    setLoadStatus('Processing on the server…');
    setLoadProgressIndeterminate('Processing on the server…');

    // Only if asked. Detection reads every message you sent, and nothing
    // here has measured how long that takes, so it is never implied by the
    // act of uploading.
    if(runDetection){
      setLoadStatus('Scanning your messages for flags…');
      await runDetectionPass(token);
      setLoadProgressIndeterminate('Processing on the server…');
    }

    const exportRes = await fetch(`${API_BASE}/export`, {
      headers: { 'Authorization': `Bearer ${token}` },
    });
    if(!exportRes.ok) throw new Error(await describeFailure('reading back the processed export', exportRes));
    const { export_url } = await exportRes.json();

    const downloadRes = await fetch(`${API_BASE}${export_url}`);
    if(!downloadRes.ok) throw new Error(await describeFailure('downloading the processed export', downloadRes));
    const downloadEta = makeRateEstimator(3000);
    const text = await readBodyWithProgress(downloadRes, (loaded, total) => {
      if(total){
        const pct = Math.round((loaded / total) * 100);
        uploadFill.style.width = pct + '%';
        const eta = downloadEta(loaded, total);
        uploadLabel.textContent =
          `Receiving your processed timeline — ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
      } else {
        // No Content-Length: report what has actually arrived rather than
        // inventing a proportion of an unknown whole.
        uploadFill.style.width = '100%';
        uploadLabel.textContent = `Receiving your processed timeline — ${formatBytes(loaded)} so far`;
      }
    });

    setLoadProgressIndeterminate('Preparing the timeline…');

    if(!applyExportText(text)){
      setLoadStatus('That file parsed, but contained no conversations — is it the right export?', true);
      failLoadProgress();
      return;
    }
    hideLoadProgress();
  } catch(err){
    console.error(err);
    failLoadProgress();
    // A TypeError here (not an HTTP error response -- those are handled by
    // describeFailure, in infra/api-client.js) means fetch() itself couldn't reach the
    // server at all -- almost always because it isn't running.
    const hint = err instanceof TypeError
      ? ` Is the backend running (cargo run -p timeline-api) at ${API_BASE}?`
      : '';
    setLoadStatus('Could not load that file through the backend — ' + err.message + hint, true);
  }
}
