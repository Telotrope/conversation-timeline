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
import { errorKindOf, errorStatusOf, PageError } from '../core/page-error.js';
import { noteMainShown } from '../core/activity-sink.js';
import { API_BASE, apiFetch, clearAuthToken, downloadSignedExport, ensureAuthToken, fetchUploadStatus, putWithProgress, requestFailure, serverUrl, signedInLabel, usesRealLogin } from '../infra/api-client.js';
import { waitForProcessing, waitMessageId } from '../core/upload-wait.js';
import { applyLocationHash } from './router.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList } from './views/conversations.js';
import { renderSubtitle } from './views/header.js';
import { renderReviewTable } from './views/review.js';
import { failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadProgressLabel, setLoadProgressMeasured, setLoadStatus, setSaveStatus, showLoadProgress, showRestoredNotice } from './widgets/status-indicators.js';

// Applies an already-downloaded export, plus the flag handles from the same
// GET /export reply: parses it, replaces the page's state, and shows the
// timeline. Returns false if the export held no
// conversations, leaving the caller to report that however suits it.
//
// Shared by the upload path and the restore-on-load path below, so a
// restored session goes through exactly the same rendering as a fresh
// upload rather than a parallel copy that can drift.
function applyExportText(text, flagHandles){
  const parsed = parseUploadedConversations(text);
  if(parsed.conversations.length === 0) return false;

  state.conversations = parsed.conversations;
  state.messages = parsed.messages;
  state.humanMessages = parsed.humanMessages;
  state.humanById = new Map(state.humanMessages.map(m => [m.id, m]));
  state.blocks = buildBlocks();
  state.rawData = parsed.rawData;
  state.flagHandles = flagHandles;

  // Your confirmed flags come from whatever the server's export embedded
  // (overrides you PATCHed to the backend earlier -- see
  // patchFlagsToBackend). There's no other recovery mechanism; see the
  // migration plan's V2a.
  state.overrides = { ...parsed.embeddedOverrides };
  const embeddedCount = Object.keys(parsed.embeddedOverrides).length;

  attachFlags();
  if(embeddedCount > 0){
    setSaveStatus('flags.loaded', { count: embeddedCount });
  }

  document.getElementById('loadScreen').hidden = true;
  document.getElementById('mainContent').hidden = false;
  noteMainShown(true);

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
//
// With a real sign-in, the remembered name is your account: being signed in
// is enough to try.
export async function tryRestoreSession(){
  let sub = null;
  if(usesRealLogin()){
    sub = await signedInLabel();
    if(!sub) return;
  } else {
    try{ sub = localStorage.getItem('timeline_dev_sub'); } catch(e){ return; }
    if(!sub) return;
    document.getElementById('devLoginSub').value = sub;
  }
  try{
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
    const exportRes = await apiFetch('/export', { token });
    if(!exportRes.ok) return;
    const { export_url, flag_handles } = await exportRes.json();
    const { res: downloadRes, text } = await downloadSignedExport(serverUrl(export_url), () => {});
    if(!downloadRes.ok) return;
    if(!applyExportText(text, flag_handles)) return;

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
  const limit = 5;
  for(;;){
    const res = await apiFetch('/detect', {
      method: 'POST',
      token,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ offset, limit }),
      facts: { offset, limit },
    });
    if(!res.ok) throw await requestFailure('scanning your messages', res);
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
    setLoadStatus('load.choose_file');
    hideLoadProgress();
    return;
  }

  try{
    showLoadProgress();
    setLoadStatus('load.signing_in');
    setLoadProgressIndeterminate('progress.signing_in');
    const token = await ensureAuthToken(document.getElementById('devLoginSub').value.trim());

    setLoadStatus('load.sending');
    setLoadProgressIndeterminate('progress.reading_file');
    const rawText = await convFile.text();
    const createRes = await apiFetch('/uploads', { method: 'POST', token, facts: { scan: runDetection } });
    if(!createRes.ok) throw await requestFailure('starting the upload', createRes);
    const { upload_id, upload_url } = await createRes.json();

    const uploadFill = document.getElementById('loadProgressFill');
    const uploadLabel = document.getElementById('loadProgressLabel');
    const uploadEta = makeRateEstimator(3000);
    setLoadProgressMeasured();
    const putRes = await putWithProgress(serverUrl(upload_url), rawText, (loaded, total) => {
      const pct = Math.round((loaded / total) * 100);
      uploadFill.style.width = pct + '%';
      const eta = uploadEta(loaded, total);
      uploadLabel.textContent =
        `Sending your file — ${formatBytes(loaded)} of ${formatBytes(total)} (${pct}%)` + (eta ? ` — ${eta}` : '');
    });
    if(!putRes.ok){
      throw new PageError(`uploading the file failed (${putRes.status})${putRes.text ? ': ' + putRes.text : ''}`, 'server_error', putRes.status);
    }

    // The bytes being sent is not the end of the wait: the server still has
    // to parse, dedup and store the upload. Saying so beats a full bar that
    // looks stuck. On AWS that happens separately, after the file lands in
    // S3, so the page asks until it's done; locally the first answer is
    // already "ready". See core/upload-wait.js.
    //
    // On AWS each answer may say which attempt is running and why the last
    // one failed; the line under the bar shows that with a clock that ticks
    // every second, so a retry doesn't look like a hang (plan
    // 2026-10-02-upload-processing-failures.md §3).
    setLoadStatus('load.processing');
    setLoadProgressIndeterminate('progress.processing');
    const waitStarted = Date.now();
    let lastAnswer = null;
    const showWait = () => {
      if(lastAnswer){
        setLoadProgressLabel(waitMessageId(lastAnswer), {
          answer: lastAnswer, elapsedMs: Date.now() - waitStarted,
          attempt: lastAnswer.attempt, max_attempts: lastAnswer.max_attempts,
        });
      }
    };
    const clock = setInterval(showWait, 1000);
    try{
      await waitForProcessing({
        fetchStatus: () => fetchUploadStatus(token, upload_id),
        sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
        now: () => Date.now(),
        onAnswer: (answer) => {
          if(answer.status === 'processing'){ lastAnswer = answer; showWait(); }
        },
      });
    } finally {
      clearInterval(clock);
    }

    // Only if asked. Detection reads every message you sent, and nothing
    // here has measured how long that takes, so it is never implied by the
    // act of uploading.
    if(runDetection){
      setLoadStatus('load.scanning');
      await runDetectionPass(token);
      setLoadProgressIndeterminate('progress.processing');
    }

    const exportRes = await apiFetch('/export', { token });
    if(!exportRes.ok) throw await requestFailure('reading back the processed export', exportRes);
    const { export_url, flag_handles } = await exportRes.json();

    const downloadEta = makeRateEstimator(3000);
    // The bar becomes a measured one once the download's bytes start
    // arriving, as it did when this read the body itself.
    let measuring = false;
    const { res: downloadRes, text } = await downloadSignedExport(serverUrl(export_url), (loaded, total) => {
      if(!measuring){ measuring = true; setLoadProgressMeasured(); }
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
    if(!downloadRes.ok) throw await requestFailure('downloading the processed export', downloadRes);

    setLoadProgressIndeterminate('progress.preparing');

    if(!applyExportText(text, flag_handles)){
      setLoadStatus('load.no_conversations');
      failLoadProgress();
      return;
    }
    hideLoadProgress();
  } catch(err){
    console.error(err);
    failLoadProgress();
    // A TypeError here (not an HTTP error response -- those are handled by
    // requestFailure, in infra/api-client.js) means fetch() itself couldn't reach the
    // server at all -- almost always because it isn't running.
    const hint = err instanceof TypeError
      ? ` Is the backend running (cargo run -p timeline-api) at ${API_BASE}?`
      : '';
    setLoadStatus('load.failed', {
      detail: err.message, hint, status: errorStatusOf(err), error_kind: errorKindOf(err),
    });
  }
}
