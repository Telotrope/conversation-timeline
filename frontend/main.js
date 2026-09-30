import { buildBlocks } from './core/blocks.js';
import { FORMAT_VERSION, parseUploadedConversations } from './core/export-format.js';
import { attachFlags, effectiveFlag } from './core/flags.js';
import { formatBytes } from './core/format.js';
import { state } from './core/state.js';
import { API_BASE, clearAuthToken, describeFailure, ensureAuthToken, patchFlagsToBackend, putWithProgress, readBodyWithProgress } from './infra/api-client.js';
import { rememberLocation, whileApplyingHash } from './ui/navigation/location.js';
import { switchTab } from './ui/navigation/tabs.js';
import { runAnalysis } from './ui/views/analytics.js';
import { renderCalendar } from './ui/views/calendar.js';
import { renderConvList, selectConversation } from './ui/views/conversations.js';
import { renderSubtitle } from './ui/views/header.js';
import { renderReviewTable, setFlagEditHandlers, showFirstReviewPage } from './ui/views/review.js';
import { failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadStatus, setSaveStatus, showLoadProgress, showRestoredNotice } from './ui/widgets/status-indicators.js';

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
async function tryRestoreSession(){
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

async function handleLoadClick(){
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

    setLoadStatus('Uploading your conversation export…');
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
        `Uploading ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
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
    setLoadStatus('Processing your export…');
    setLoadProgressIndeterminate('Finishing up on the server…');

    // Only if asked. Detection reads every message you sent, and nothing
    // here has measured how long that takes, so it is never implied by the
    // act of uploading.
    if(runDetection){
      setLoadStatus('Scanning your messages for flags…');
      await runDetectionPass(token);
      setLoadProgressIndeterminate('Finishing up on the server…');
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
          `Downloading ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
      } else {
        // No Content-Length: report what has actually arrived rather than
        // inventing a proportion of an unknown whole.
        uploadFill.style.width = '100%';
        uploadLabel.textContent = `Downloading… ${formatBytes(loaded)} so far`;
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
    // describeFailure above) means fetch() itself couldn't reach the
    // server at all -- almost always because it isn't running.
    const hint = err instanceof TypeError
      ? ` Is the backend running (cargo run -p timeline-api) at ${API_BASE}?`
      : '';
    setLoadStatus('Could not load that file through the backend — ' + err.message + hint, true);
  }
}

document.getElementById('loadBtn').addEventListener('click', handleLoadClick);
document.getElementById('loadDifferentBtn').addEventListener('click', ()=>{
  document.getElementById('mainContent').style.display = 'none';
  document.getElementById('loadScreen').style.display = '';
  document.getElementById('loadConvFile').value = '';
  setLoadStatus('');
  // Asking for a different file is also how you say "stop bringing the old
  // one back", so the remembered session goes with it. Without this, the
  // next reload would silently restore exactly what you just dismissed.
  try{ localStorage.removeItem('timeline_dev_sub'); } catch(e){ /* nothing to forget */ }
  clearAuthToken();
  window.location.hash = '';
});

state.blocks = buildBlocks();
attachFlags();

// Clicking any single checkbox, or the Approve button, promotes ALL THREE
// flags on that message to explicit user values at once — using the
// just-changed value for `changedType` (if any) and the message's current
// effective value for the other two. This is what "approving a row" means:
// one click reviews the whole message, not just the box you touched.
function setRowOverrides(id, changedType, changedValue){
  const msg = state.humanById.get(id);
  if(!msg) return;
  const values = {
    caps: changedType === 'caps' ? changedValue : effectiveFlag(msg, 'caps'),
    angry: changedType === 'angry' ? changedValue : effectiveFlag(msg, 'angry'),
    critical: changedType === 'critical' ? changedValue : effectiveFlag(msg, 'critical'),
  };
  state.overrides[id] = values;
  attachFlags();
  patchFlagsToBackend(msg, values).then(setSaveStatus);
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  renderReviewTable();
}

function approveRow(id){
  setRowOverrides(id, null, null);
}

// Writes both the auto-detected values and your confirmed overrides onto
// each message (in two separate, namespaced fields, so a future load can
// never confuse one for the other), wraps the whole thing with a format
// version marker, and downloads it. This is now the only save mechanism —
// one self-contained file carries the conversation data, the automatic
// tags, and your corrections together.
function exportAnnotatedConversations(){
  if(!state.rawData){
    setSaveStatus('No conversation data loaded to annotate.');
    return;
  }
  // Mutate state.rawData directly rather than deep-cloning it first — for a
  // file this size, a stringify-then-reparse clone briefly needs 2-3x the
  // data's size in memory all at once (original + serialized string +
  // freshly parsed copy), which is enough to crash the tab outright on a
  // large export. There's nothing unsafe about mutating in place here:
  // we only ever add two clearly namespaced fields to human messages,
  // never remove or alter anything else, so doing it again on a later
  // export is harmless and idempotent.
  const annotated = state.rawData;
  annotated.forEach((c, convIdx) => {
    (c.chat_messages || []).forEach(m => {
      if(m.sender !== 'human') return;
      const id = convIdx + '|' + m.created_at;
      delete m._claude_timeline_flags; // retire the old single-field format

      const msg = state.humanById.get(id);
      if(msg){
        m._claude_timeline_auto = {
          caps: msg.default_caps,
          angry: msg.default_angry,
          critical: msg.default_critical,
          source: msg.auto_source || 'heuristic',
        };
      }

      if(state.overrides[id] && Object.keys(state.overrides[id]).length){
        m._claude_timeline_user = state.overrides[id];
      } else {
        delete m._claude_timeline_user;
      }
    });
  });

  const wrapped = { claude_timeline_format_version: FORMAT_VERSION, conversations: annotated };

  const blob = new Blob([JSON.stringify(wrapped)], {type:'application/json'});
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = 'conversations-with-flags.json';
  a.click();
  URL.revokeObjectURL(url);
  setSaveStatus('Downloaded conversations-with-flags.json — load this file directly next time.');
}

function applyLocationHash(){
  const raw = window.location.hash.replace(/^#/, '');
  if(!raw) return;
  const [tab, arg] = raw.split('/');
  if(!document.getElementById('view-' + tab)) return;

  whileApplyingHash(()=>{
    switchTab(tab);
    if(tab === 'conversations' && arg !== undefined){
      const idx = parseInt(arg, 10);
      if(!isNaN(idx) && idx >= 0 && idx < state.conversations.length) selectConversation(idx);
    }
    if(tab === 'analytics' && arg){
      const btn = document.querySelector(`.analytics-item[data-analysis="${arg}"]`);
      if(btn) runAnalysis(arg, {});
    }
  });
}

window.addEventListener('hashchange', applyLocationHash);
document.querySelectorAll('nav.tabs button').forEach(b=>{
  b.addEventListener('click', ()=>{ switchTab(b.dataset.tab); rememberLocation(); });
});

document.getElementById('convSearch').addEventListener('input', (e)=> renderConvList(e.target.value));

document.getElementById('reviewSearch').addEventListener('input', showFirstReviewPage);
document.getElementById('reviewFilter').addEventListener('change', showFirstReviewPage);
setFlagEditHandlers(setRowOverrides, approveRow);
document.getElementById('exportAnnotatedBtn').addEventListener('click', exportAnnotatedConversations);

function onVisibilityToggleChanged(){
  state.showAuto = document.getElementById('toggleShowAuto').checked;
  state.showUser = document.getElementById('toggleShowUser').checked;
  attachFlags();
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  if(state.selectedConversation !== null) selectConversation(state.selectedConversation);
  renderReviewTable();
}
document.getElementById('toggleShowAuto').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowUser').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowReplies').addEventListener('change', ()=>{
  state.showReplies = document.getElementById('toggleShowReplies').checked;
  renderReviewTable();
});

document.querySelectorAll('.analytics-item').forEach(btn=>{
  btn.addEventListener('click', ()=> runAnalysis(btn.dataset.analysis, {}));
});

// Last thing in the file, so everything it calls already exists. Deliberately
// not awaited: the load screen is already usable, and a slow or unreachable
// backend must not hold the page hostage while it decides there is nothing to
// restore.
tryRestoreSession();
