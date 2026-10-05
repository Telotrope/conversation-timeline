// Getting data onto the screen (plan docs/plans/2026-10-05-screen-flow.md §6,
// §6b): the Upload page, which sends one or more files, waits for the server
// to process them and optionally runs the scan; the step functions it and
// the loading modal are built from; and drawing the downloaded timeline.
//
// Which page shows next is ui/page-flow.js's decision. The Upload page
// reports to it through the `flow` main.js connects (connectUploadPage);
// nothing here imports it, since modules at this level don't import each
// other (frontend/tests/structure.test.js).

import { buildBlocks } from '../core/blocks.js';
import { parseUploadedConversations } from '../core/export-format.js';
import { attachFlags } from '../core/flags.js';
import { formatBytes } from '../core/format.js';
import { state } from '../core/state.js';
import { errorKindOf, errorStatusOf, PageError } from '../core/page-error.js';
import { startBatch } from '../core/upload-batch.js';
import { API_BASE, apiFetch, downloadSignedExport, ensureAuthToken, fetchConversationRecords, fetchUploadStatus, putWithProgress, requestFailure, serverUrl, signedInLabel, usesRealLogin } from '../infra/api-client.js';
import { waitForProcessing } from '../core/upload-wait.js';
import { renderCalendar } from './views/calendar.js';
import { renderConvList } from './views/conversations.js';
import { renderFiles } from './views/files.js';
import { renderSubtitle } from './views/header.js';
import { renderReviewTable } from './views/review.js';
import { addFileLine, clearFileLines, failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadProgressMeasured, setLoadStatus, setSaveStatus, showDownloadProgress, showLoadProgress, showScanProgress, showSendProgress, showWaitProgress } from './widgets/status-indicators.js';

// Draws an already-downloaded export, with the flag handles from the same
// GET /export reply and the conversations' records: parses it, replaces the
// page's state and draws every view. Returns false if the export held no
// conversations, leaving the caller to report that however suits it. Which
// page is shown is the caller's business.
export function applyExportText(text, flagHandles, records, uploads){
  const parsed = parseUploadedConversations(text);
  if(parsed.conversations.length === 0) return false;
  state.records = new Map(records.map((r) => [r.conversation_id, r]));
  state.uploads = uploads;
  state.conversations = parsed.conversations.map((c, i) => ({
    ...c, id: parsed.conversationIds[i], untimed: parsed.untimedCounts[i],
  }));
  state.messages = parsed.messages;
  state.humanMessages = parsed.humanMessages;
  state.humanById = new Map(state.humanMessages.map(m => [m.id, m]));
  state.rawData = parsed.rawData;
  state.flagHandles = flagHandles;
  // Your confirmed flags come from whatever the server's export embedded
  // (overrides you PATCHed to the backend earlier -- see
  // patchFlagsToBackend). There's no other recovery mechanism; see the
  // migration plan's V2a.
  state.overrides = { ...parsed.embeddedOverrides };
  const embeddedCount = Object.keys(parsed.embeddedOverrides).length;
  redrawTimeline();
  if(embeddedCount > 0) setSaveStatus('flags.loaded', { count: embeddedCount });
  return true;
}

// Rebuilds the sessions (a changed start or end moves a conversation whose
// messages have no times) and draws every view.
export function redrawTimeline(){
  state.blocks = buildBlocks();
  attachFlags();
  renderSubtitle();
  renderCalendar();
  renderConvList(document.getElementById('convSearch').value);
  renderReviewTable();
  renderFiles();
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

// --- The Upload page ---

// The files chosen so far. Kept here rather than read from the file input,
// whose list can't have a file taken out of it.
let CHOSEN = [];
let FLOW = null;
let BATCH = null;

// flow: { hasData(), dataArrived(), toSignIn(messageId), afterUpload(result) }, from
// ui/page-flow.js through main.js.
export function connectUploadPage(flow){
  FLOW = flow;
}

// Whether files are being sent or processed now.
export function uploading(){
  return BATCH !== null;
}

// Empties the page for a new batch: no files, no messages, no bar. "Back to
// timeline" shows only when there is a timeline to go back to.
export function resetUploadPage(hasData){
  CHOSEN = [];
  document.getElementById('loadConvFile').value = '';
  showChosenFiles();
  setLoadStatus(null);
  hideLoadProgress();
  clearFileLines();
  showUploadButtons(false, hasData);
}

function showUploadButtons(sending, hasData){
  document.getElementById('loadBtn').hidden = sending;
  document.getElementById('stopBtn').hidden = !sending;
  document.getElementById('uploadBackBtn').hidden = sending || !hasData;
}

// The file input's choice joins the list, which shows each file's name and
// size with a Remove button.
export function chooseFiles(){
  CHOSEN = CHOSEN.concat(Array.from(document.getElementById('loadConvFile').files));
  showChosenFiles();
}

function showChosenFiles(){
  const list = document.getElementById('chosenFiles');
  list.replaceChildren(...CHOSEN.map((file, i) => {
    const li = document.createElement('li');
    const name = document.createElement('span');
    name.textContent = file.name;
    const size = document.createElement('span');
    size.className = 'size';
    size.textContent = formatBytes(file.size);
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.className = 'btn-secondary btn-small';
    remove.textContent = 'Remove';
    remove.addEventListener('click', () => { CHOSEN.splice(i, 1); showChosenFiles(); });
    li.append(name, size, remove);
    return li;
  }));
}

export function stopUpload(){
  if(BATCH) BATCH.stop();
}

// Upload pressed: sign-in token, every file at once, the scan if ticked,
// then on to Describe (through the flow) with what was processed.
export async function handleUploadClick(){
  const files = CHOSEN.slice();
  const scan = document.getElementById('autoDetectCheckbox').checked;
  if(!files.length){
    setLoadStatus('load.choose_file');
    hideLoadProgress();
    return;
  }
  clearFileLines();
  showLoadProgress();
  const token = await signInForUpload();
  if(!token) return;
  showUploadButtons(true, false);
  const result = await sendBatch(token, files, scan);
  if(result.processed.length > 0) FLOW.dataArrived();
  showUploadButtons(false, FLOW.hasData());
  if(result.processed.length === 0) return reportNothingProcessed(files, result);
  if(await finishBatch(token, scan)) FLOW.afterUpload({ token, files, ...result });
}

async function signInForUpload(){
  setLoadStatus('load.signing_in');
  setLoadProgressIndeterminate('progress.signing_in');
  try{
    return await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
  } catch(err){
    return failUpload(err);
  }
}

// A failure the whole batch shares, on the status line; an expired
// sign-in goes back to the Sign-in page instead.
function failUpload(err){
  console.error(err);
  if(errorKindOf(err) === 'not_logged_in' && usesRealLogin()){
    FLOW.toSignIn('signIn.ran_out');
    return null;
  }
  failLoadProgress();
  setLoadStatus(...describeLoadFailure(err));
  return null;
}

// Sends and waits for every file at once; one bar for the bytes sent across
// all of them, then the wait for the server. Resolves to the batch's result.
async function sendBatch(token, files, scan){
  setLoadStatus('load.sending');
  setLoadProgressIndeterminate('progress.reading_file');
  const human = await humanName();
  const eta = makeRateEstimator(3000);
  const wait = waitingDisplay(files.length);
  BATCH = startBatch(files, (file, ctx) => uploadOneFile(token, file, scan, human, {
    sent: ctx.sent, sendingDone: () => { ctx.sendingDone(); wait.oneSent(); },
    answer: wait.answer, registerAbort: ctx.registerAbort, sleep: ctx.sleep,
  }), {
    progress: (loaded, total) => { setLoadProgressMeasured(); showSendProgress(loaded, total, eta(loaded, total)); },
    failed: (index, error) => { wait.oneSent(); addFileLine('load.file_failed', failureLine(files[index], error)); },
  });
  try{
    const result = await BATCH.done;
    result.stopped.forEach((i) => addFileLine('load.file_stopped', { file: files[i].name }));
    return result;
  } finally {
    BATCH = null;
    wait.stop();
  }
}

// The wait for the server, once every file's bytes are sent: a moving bar
// and a clock, with the latest answer's attempt and error.
function waitingDisplay(count){
  let left = count;
  let started = 0;
  let last = null;
  const show = () => { if(last) showWaitProgress(last, Date.now() - started); };
  const clock = setInterval(show, 1000);
  return {
    oneSent: () => {
      left -= 1;
      if(left !== 0) return;
      started = Date.now();
      setLoadStatus('load.processing');
      setLoadProgressIndeterminate('progress.processing');
    },
    answer: (answer) => { last = answer; show(); },
    stop: () => clearInterval(clock),
  };
}

function failureLine(file, error){
  const [, values] = describeLoadFailure(error);
  return { ...values, file: file.name };
}

// Nothing processed: every file failed, or Stop came first. One file's
// failure reads as it always has; several are summed up, their reasons
// listed below the bar.
function reportNothingProcessed(files, result){
  failLoadProgress();
  if(result.failed.length === 0) setLoadStatus('load.stopped_none');
  else if(files.length === 1) setLoadStatus(...describeLoadFailure(result.failed[0].error));
  else setLoadStatus('load.none_succeeded', { count: files.length });
}

// The scan, if ticked, then a check that the user has any conversations at
// all: a file of none leaves nothing to describe or show. Resolves to
// whether to go on to Describe.
async function finishBatch(token, scan){
  try{
    if(scan){
      setLoadStatus('load.scanning');
      await runDetectionPass(token, showScanProgress);
    }
    if((await fetchConversationRecords(token)).length === 0){
      setLoadStatus('load.no_conversations');
      failLoadProgress();
      return false;
    }
  } catch(err){
    failUpload(err);
    return false;
  }
  hideLoadProgress();
  setLoadStatus(null);
  return true;
}
