// Getting data onto the screen: upload the file you pick, optionally run the
// backend's flag detection page by page with progress, download the processed
// export, hand it to core/export-format.js, and draw every view. On page load
// it also reopens your last session if the backend still has it, then opens
// the view named in the web address.

import { buildBlocks } from '../core/blocks.js';
import { parseUploadedConversations } from '../core/export-format.js';
import { attachFlags } from '../core/flags.js';
import { state } from '../core/state.js';
import { errorKindOf, errorStatusOf, PageError } from '../core/page-error.js';
import { noteMainShown } from '../core/activity-sink.js';
import { API_BASE, apiFetch, clearAuthToken, downloadSignedExport, ensureAuthToken, fetchUploadStatus, putWithProgress, requestFailure, serverUrl, signedInLabel, usesRealLogin } from '../infra/api-client.js';
import { waitForProcessing } from '../core/upload-wait.js';
import { applyLocationHash } from './router.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList } from './views/conversations.js';
import { renderSubtitle } from './views/header.js';
import { renderReviewTable } from './views/review.js';
import { failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadProgressMeasured, setLoadStatus, setSaveStatus, showDownloadProgress, showLoadProgress, showRestoredNotice, showScanProgress, showSendProgress, showWaitProgress } from './widgets/status-indicators.js';

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

// --- One function per step of getting a file onto the screen ---
// None of these touches the page: each returns its result or throws, and
// reports progress through the callback it is given. The callers below set
// the bar and its text (ui/widgets/status-indicators.js), so each caller
// reads as a short list of steps (plan
// docs/plans/2026-10-05-screen-flow.md §6).

// Starts an upload: POST /uploads with what the file itself can't say (its
// name, when it was last written, and who the human is), so the server can
// fill in each conversation's first guess. Resolves to { upload_id,
// upload_url }.
export async function startUpload(token, file, scan, humanName){
  const body = { file_name: file.name, human_name: humanName };
  if(file.lastModified) body.file_written_at = new Date(file.lastModified).toISOString();
  const res = await apiFetch('/uploads', {
    method: 'POST',
    token,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    facts: { scan },
  });
  if(!res.ok) throw await requestFailure('starting the upload', res);
  return res.json();
}

// Sends the file's text to the signed address. onProgress(loaded, total);
// registerAbort(fn) receives a function that cancels the send.
export async function sendFile(uploadUrl, text, onProgress, registerAbort){
  const res = await putWithProgress(serverUrl(uploadUrl), text, onProgress, registerAbort);
  if(!res.ok){
    throw new PageError(`uploading the file failed (${res.status})${res.text ? ': ' + res.text : ''}`, 'server_error', res.status);
  }
}

// Waits until the server has processed the upload. The bytes arriving is
// not the end of the wait: the server still parses, dedups and stores it
// (on AWS separately, after the file lands in S3, so the page asks until
// it's done; locally the first answer is already "ready"). onAnswer gets
// each "processing" answer, which on AWS may say which attempt is running
// and why the last one failed. `sleep` is passed in, so a caller can end
// the wait early (Stop).
export function waitUntilProcessed(token, uploadId, onAnswer, sleep){
  return waitForProcessing({
    fetchStatus: () => fetchUploadStatus(token, uploadId),
    sleep,
    now: () => Date.now(),
    onAnswer: (answer) => { if(answer.status === 'processing') onAnswer(answer); },
  });
}

// One file, start to processed: start the upload, send it, wait for it.
// Resolves to its upload id. `on` holds the callbacks: sent(loaded, total),
// sendingDone(), answer(processingAnswer), registerAbort(fn), and sleep(ms).
export async function uploadOneFile(token, file, scan, humanName, on){
  const text = await file.text();
  const { upload_id: uploadId, upload_url: uploadUrl } = await startUpload(token, file, scan, humanName);
  await sendFile(uploadUrl, text, on.sent, on.registerAbort);
  on.sendingDone();
  await waitUntilProcessed(token, uploadId, on.answer, on.sleep);
  return uploadId;
}

// Runs the backend's non-generative detection pass, one page of
// conversations at a time. The server could do the whole thing in one
// request, but then there would be nothing to report: paging is what makes
// the progress bar show real, earned progress rather than a spinner.
// onProgress(covered, total) after each page; resolves to how many
// messages were scanned.
export async function runDetectionPass(token, onProgress){
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
    const done = body.next_offset === null || body.next_offset === undefined;
    onProgress(done ? body.total_conversations : body.next_offset, body.total_conversations);
    if(done) return detected;
    offset = body.next_offset;
  }
}

// Downloads the user's whole processed timeline: GET /export, then the
// signed link it hands back. onProgress(loaded, total) as bytes arrive
// (total undefined when the answer doesn't say). Resolves to { text,
// flagHandles }.
export async function downloadTimeline(token, onProgress){
  const exportRes = await apiFetch('/export', { token });
  if(!exportRes.ok) throw await requestFailure('reading back the processed export', exportRes);
  const { export_url, flag_handles } = await exportRes.json();
  const { res, text } = await downloadSignedExport(serverUrl(export_url), onProgress);
  if(!res.ok) throw await requestFailure('downloading the processed export', res);
  return { text, flagHandles: flag_handles };
}

// A failure as the status line's message id and values. A TypeError (not
// an HTTP error answer -- those come through requestFailure) means fetch()
// couldn't reach the server at all, almost always because it isn't
// running, so the message says where it was looked for.
export function describeLoadFailure(err){
  const hint = err instanceof TypeError
    ? ` Is the backend running (cargo run -p timeline-api) at ${API_BASE}?`
    : '';
  return ['load.failed', {
    detail: err.message, hint, status: errorStatusOf(err), error_kind: errorKindOf(err),
  }];
}

// Who the guessed metadata names as the human: the signed-in account, or
// the dev login name locally.
export async function humanName(){
  if(usesRealLogin()) return (await signedInLabel()) || 'You';
  return document.getElementById('devLoginSub').value.trim() || 'You';
}

// The download, with its bar: measured once bytes start arriving.
export function downloadWithBar(token){
  const eta = makeRateEstimator(3000);
  let measuring = false;
  return downloadTimeline(token, (loaded, total) => {
    if(!measuring){ measuring = true; setLoadProgressMeasured(); }
    showDownloadProgress(loaded, total, total ? eta(loaded, total) : '');
  });
}

export async function handleLoadClick(){
  const convFile = document.getElementById('loadConvFile').files[0];
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
    const sendEta = makeRateEstimator(3000);
    const waitStarted = { at: 0 };
    let lastAnswer = null;
    const showWait = () => { if(lastAnswer) showWaitProgress(lastAnswer, Date.now() - waitStarted.at); };
    const clock = setInterval(showWait, 1000);
    try{
      await uploadOneFile(token, convFile, runDetection, await humanName(), {
        sent: (loaded, total) => { setLoadProgressMeasured(); showSendProgress(loaded, total, sendEta(loaded, total)); },
        sendingDone: () => {
          waitStarted.at = Date.now();
          setLoadStatus('load.processing');
          setLoadProgressIndeterminate('progress.processing');
        },
        answer: (answer) => { lastAnswer = answer; showWait(); },
        registerAbort: () => {},
        sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
      });
    } finally {
      clearInterval(clock);
    }
    // Only if asked. Detection reads every message you sent, and nothing
    // here has measured how long that takes, so it is never implied by the
    // act of uploading.
    if(runDetection){
      setLoadStatus('load.scanning');
      await runDetectionPass(token, showScanProgress);
      setLoadProgressIndeterminate('progress.processing');
    }
    const { text, flagHandles } = await downloadWithBar(token);
    setLoadProgressIndeterminate('progress.preparing');
    if(!applyExportText(text, flagHandles)){
      setLoadStatus('load.no_conversations');
      failLoadProgress();
      return;
    }
    hideLoadProgress();
  } catch(err){
    console.error(err);
    failLoadProgress();
    setLoadStatus(...describeLoadFailure(err));
  }
}
