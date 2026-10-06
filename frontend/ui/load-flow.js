// Getting data onto the server (plan docs/plans/2026-10-05-screen-flow.md
// §6, §6b): the Upload page, which prepares one or more files in the
// browser, sends them, waits for the server to process them and optionally
// runs the scan; and the step functions it is built from.
//
// Which page shows next is ui/page-flow.js's decision. The Upload page
// reports to it through the `flow` main.js connects (connectUploadPage);
// nothing here imports it, since modules at this level don't import each
// other (frontend/tests/structure.test.js).

import { formatBytes } from '../core/format.js';
import { errorKindOf, errorStatusOf, PageError } from '../core/page-error.js';
import { startBatch } from '../core/upload-batch.js';
import { API_BASE, apiFetch, ensureAuthToken, fetchConversationsPart, fetchUploadStatus, postDetect, putWithProgress, requestFailure, serverUrl, signedInLabel, usesRealLogin } from '../infra/api-client.js';
import { prepareUpload } from '../infra/upload-preparer.js';
import { waitForProcessing } from '../core/upload-wait.js';
import { addFileLine, clearFileLines, failLoadProgress, hideLoadProgress, makeRateEstimator, setLoadProgressIndeterminate, setLoadStatus, showLoadProgress, showPrepareProgress, showScanProgress, showSendProgress, showWaitProgress } from './widgets/status-indicators.js';

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

// Sends the prepared file (a Blob) to the signed address.
// onProgress(loaded, total); registerAbort(fn) receives a function that
// cancels the send.
export async function sendFile(uploadUrl, body, onProgress, registerAbort){
  const res = await putWithProgress(serverUrl(uploadUrl), body, onProgress, registerAbort);
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

// Whether files are sent as they are, without slimming (plan §7b): only
// when the page's address asks, with ?upload=unslimmed. A browser test of
// the server's size limit needs a large file to reach the server large.
export function sendsUnslimmed(){
  return new URLSearchParams(window.location.search).get('upload') === 'unslimmed';
}

// One file, start to processed: prepare it, start the upload, send it,
// wait for it. Resolves to its upload id. `on` holds the callbacks:
// prepared({ read, size, conversations, compressed }), ready(bytesToSend),
// sent(loaded, total), sendingDone(), answer(processingAnswer),
// registerAbort(fn), and sleep(ms); and, for a batch, turn() (resolves when
// this file may start its upload) and sending() (called as it starts
// sending), so files are sent in their order however long each took to
// prepare.
export async function uploadOneFile(token, file, scan, humanName, on){
  const { turn = async () => {}, sending = () => {} } = on;
  const prepared = sendsUnslimmed()
    ? { body: file.slice(0, file.size) }
    : await prepareUpload(file, on.prepared, on.registerAbort);
  on.ready(prepared.body.size);
  await turn();
  const { upload_id: uploadId, upload_url: uploadUrl } = await startUpload(token, file, scan, humanName);
  const sent = sendFile(uploadUrl, prepared.body, on.sent, on.registerAbort);
  sending();
  await sent;
  on.sendingDone();
  await waitUntilProcessed(token, uploadId, on.answer, on.sleep);
  return uploadId;
}

// Runs the backend's non-generative detection pass. The server works within
// its time limit and answers with how many sessions it has done of the
// total and where to carry on (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8); the page asks
// again with that cursor until it is done, so the bar shows real progress.
// The scan writes flags, so its data version changes as it goes; that is
// not a reason to start again. onProgress(done, total) after each answer;
// resolves to how many messages were scanned. stopped(), checked before
// each request, ends the pass early (the Upload page's Stop).
export async function runDetectionPass(token, onProgress, stopped = () => false){
  let cursor = null;
  let detected = 0;
  for(let part = 0; ; part += 1){
    if(stopped()) return detected;
    const body = await postDetect(token, cursor, part);
    detected += body.messages_detected;
    onProgress(body.sessions_done, body.sessions_total);
    cursor = body.cursor ?? null;
    if(cursor === null) return detected;
  }
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

// --- The Upload page ---

// The files chosen so far. Kept here rather than read from the file input,
// whose list can't have a file taken out of it.
let CHOSEN = [];
let FLOW = null;
let BATCH = null;
// From Upload pressed until the page moves on: sending, waiting and the
// scan. While it lasts only Stop shows, and Back is refused.
let BUSY = false;
let SCAN_STOPPED = false;

// flow: { hasData(), dataArrived(), toSignIn(messageId), afterUpload(result) }, from
// ui/page-flow.js through main.js.
export function connectUploadPage(flow){
  FLOW = flow;
}

// Whether files are being sent, processed or scanned now.
export function uploading(){
  return BUSY;
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

// Stop: cancels the files still being sent or processed; during the scan,
// ends it after the request under way (the files are already uploaded).
export function stopUpload(){
  if(BATCH) BATCH.stop();
  else SCAN_STOPPED = true;
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
  setBusy(true);
  const result = await sendBatch(token, files, scan);
  if(result.processed.length > 0) FLOW.dataArrived();
  const goOn = result.processed.length > 0 && await finishBatch(token, scan);
  setBusy(false);
  if(result.processed.length === 0) return reportNothingProcessed(files, result);
  if(goOn) FLOW.afterUpload({ token, files, ...result });
}

function setBusy(busy){
  BUSY = busy;
  SCAN_STOPPED = false;
  showUploadButtons(busy, FLOW.hasData());
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

// Prepares, sends and waits for every file at once; one bar across all of
// them, first for preparing (bytes read of the files' size), then for
// sending (bytes sent of what is sent), then the wait for the server.
// Resolves to the batch's result.
async function sendBatch(token, files, scan){
  setLoadStatus(sendsUnslimmed() ? 'load.sending' : 'load.preparing');
  setLoadProgressIndeterminate('progress.reading_file');
  const human = await humanName();
  const wait = waitingDisplay(files.length);
  const tally = batchTally(files);
  // startBatch's own byte count assumes each file is sent at its size;
  // prepared files are smaller, so the bar is drawn from the tally instead.
  // startBatch starts the files in order, so the n-th call is file n.
  let started = 0;
  // Each file starts sending once the one before it has (or has failed or
  // stopped), as the files were sent before preparing came first.
  const begun = files.map(() => {
    let begin;
    const promise = new Promise((resolve) => { begin = resolve; });
    return { promise, begin };
  });
  BATCH = startBatch(files, async (file, ctx) => {
    const i = started++;
    try{
      return await uploadOneFile(token, file, scan, human, {
        prepared: (p) => tally.prepared(i, p),
        ready: (size) => tally.ready(i, size),
        turn: () => (i === 0 ? Promise.resolve() : begun[i - 1].promise),
        sending: begun[i].begin,
        sent: (loaded) => tally.sent(i, loaded),
        sendingDone: () => { tally.finished(i); ctx.sendingDone(); wait.oneSent(); },
        answer: wait.answer, registerAbort: ctx.registerAbort, sleep: ctx.sleep,
      });
    } finally {
      begun[i].begin();
    }
  }, {
    progress: () => {},
    failed: (index, error) => { tally.finished(index); wait.oneSent(); addFileLine('load.file_failed', failureLine(files[index], error)); },
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

// Each file's progress through preparing and sending, drawn as one bar:
// preparing while any file still is, then sending.
function batchTally(files){
  const each = files.map((f) => ({ preparing: true, read: 0, size: f.size, conversations: 0, compressed: 0, sent: 0, body: 0 }));
  const eta = makeRateEstimator(3000);
  const sum = (key) => each.reduce((n, t) => n + t[key], 0);
  let sending = false;
  const show = () => {
    if(each.some((t) => t.preparing)){
      return showPrepareProgress({ read: sum('read'), size: sum('size'), conversations: sum('conversations'), compressed: sum('compressed') });
    }
    if(!sending){
      sending = true;
      setLoadStatus('load.sending');
    }
    const loaded = sum('sent');
    const total = sum('body');
    showSendProgress(loaded, total, eta(loaded, total));
  };
  const done = (t) => Object.assign(t, { preparing: false, read: t.size });
  return {
    prepared: (i, p) => { Object.assign(each[i], { read: p.read, conversations: p.conversations, compressed: p.compressed }); show(); },
    ready: (i, size) => { Object.assign(done(each[i]), { body: size }); show(); },
    sent: (i, loaded) => { each[i].sent = loaded; show(); },
    // Sent, failed or stopped: nothing more to wait for from this file.
    finished: (i) => { const t = done(each[i]); t.sent = t.body; },
  };
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
      setLoadProgressIndeterminate('progress.scan_starting');
      await runDetectionPass(token, showScanProgress, () => SCAN_STOPPED);
    }
    if((await fetchConversationsPart(token)).total === 0){
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
