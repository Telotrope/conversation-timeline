// Which page shows, and when: the page flow of plan
// docs/plans/2026-10-05-screen-flow.md §3, and the web address of each page
// (§4). Each function here is one of the plan's arcs: opening the page,
// signing in, uploading, describing, the timeline and its loading modal,
// signing out.
//
// The Sign-in, Upload and Describe pages' own work lives in other modules
// at this level (login-panel.js, load-flow.js, describe-form.js). Modules
// at one level may not import from each other, as
// frontend/tests/structure.test.js checks, so main.js hands their functions
// in through connectPageFlow.

import { errorKindOf, errorStatusOf } from '../core/page-error.js';
import { state } from '../core/state.js';
import { ensureAuthToken, fetchConversationRecords, fetchUploads, usesRealLogin } from '../infra/api-client.js';
import { currentPage, showPage } from './navigation/pages.js';
import { switchTab } from './navigation/tabs.js';
import { selectConversation } from './views/conversations.js';
import { closeLoadingModal, offerLoadingModalChoices, openLoadingModal } from './widgets/loading-modal.js';
import {
  failLoadProgress, hideLoadProgress, setDescribeStatus, setLoadProgressIndeterminate, setLoadStatus,
  setSignInStatus, showLoadProgress,
} from './widgets/status-indicators.js';

// From main.js: signedIn(), accountLabel(), signOut(), refreshSignIn(), resetUploadPage(hasData),
// uploading(), openDescribe(subject), downloadWithBar(token),
// applyExportText(text, flagHandles, records, uploads), redrawTimeline(),
// applyLocationHash(), describeLoadFailure(err).
let DEPS = null;
let HAS_DATA = false;
let TIMELINE_LOADED = false;
// The timeline's download, started when an upload finished and still
// running while its files are described.
let PENDING = null;
let SUBJECT = null;

export function connectPageFlow(deps){
  DEPS = deps;
}

export function hasData(){
  return HAS_DATA;
}

// The Upload page processed files: there is data now, and any timeline
// already drawn is out of date.
export function dataArrived(){
  HAS_DATA = true;
  TIMELINE_LOADED = false;
}

function token(){
  return ensureAuthToken(document.getElementById('devLoginSub').value.trim());
}

function expired(err){
  return errorKindOf(err) === 'not_logged_in' && usesRealLogin();
}

// --- Addresses (plan §4) ---

const TAB_ADDRESS = /^#(calendar|conversations|review|analytics|files)(\/|$)/;

function address(hash, push){
  if(window.location.hash === hash) return;
  if(push) window.history.pushState(null, '', hash);
  else window.history.replaceState(null, '', hash);
}

function describeAddress(subject){
  if(subject.kind === 'file') return `#describe/file/${subject.uploadId}`;
  if(subject.kind === 'conversation') return `#describe/conversation/${subject.conversationId}`;
  return '#describe';
}

function subjectFromAddress(hash){
  const m = /^#describe\/(file|conversation)\/(.+)$/.exec(hash);
  if(!m) return null;
  return m[1] === 'file' ? { kind: 'file', uploadId: m[2] } : { kind: 'conversation', conversationId: m[2] };
}

// --- Opening the page, and signing in ---

export async function start(){
  const wanted = window.location.hash;
  if(!(await DEPS.signedIn())) return toSignIn(null);
  await enterSignedIn(wanted);
}

// Signed in: Upload when there is nothing yet (or the address asks for
// it), otherwise the timeline behind its loading modal.
export async function enterSignedIn(wanted = ''){
  document.getElementById('accountName').textContent = await DEPS.accountLabel();
  let records;
  try{
    records = await fetchConversationRecords(await token());
  } catch(err){
    return checkFailed(err);
  }
  HAS_DATA = records.length > 0;
  if(wanted === '#upload' || !HAS_DATA) return toUpload(false);
  await openTimeline(records);
  const subject = subjectFromAddress(wanted);
  if(subject && TIMELINE_LOADED) toDescribe(subject, false);
}

function checkFailed(err){
  console.error(err);
  if(expired(err)) return toSignIn('signIn.ran_out');
  toSignIn('signIn.check_failed', { detail: err.message, status: errorStatusOf(err), error_kind: errorKindOf(err) });
  document.getElementById('checkAgainBtn').hidden = false;
}

export function toSignIn(messageId, values = {}){
  // The sign-in line was worked out when the page opened; a sign-in that
  // has since run out must not still read "Signed in as…".
  DEPS.refreshSignIn();
  showPage('signIn');
  setSignInStatus(messageId, values);
  document.getElementById('checkAgainBtn').hidden = true;
  address('#signin', false);
}

export async function signOut(){
  await DEPS.signOut();
  HAS_DATA = false;
  TIMELINE_LOADED = false;
  PENDING = null;
  closeLoadingModal();
  toSignIn(null);
}

// --- Upload ---

export function toUpload(push){
  showPage('upload');
  DEPS.resetUploadPage(HAS_DATA);
  address('#upload', push);
}

// The Upload page finished with at least one file processed: the timeline
// starts downloading in the background, and Describe opens for the files.
export function afterUpload({ token: t, files, processed, failed, stopped }){
  HAS_DATA = true;
  TIMELINE_LOADED = false;
  PENDING = DEPS.downloadWithBar(t);
  // Its failure is reported when the timeline opens and waits for it.
  PENDING.catch(() => {});
  const notHere = [
    ...failed.map(({ index, error }) => ({ file: files[index].name, reason: error.message })),
    ...stopped.map((index) => ({ file: files[index].name, reason: 'stopped before it was processed' })),
  ];
  toDescribe({ kind: 'batch', uploadIds: processed.map((p) => p.uploadId), notHere }, true);
}

// --- Describe ---

export function toDescribe(subject, push = true){
  SUBJECT = subject;
  showPage('describe');
  address(describeAddress(subject), push);
  DEPS.openDescribe(subject);
}

// Done (saved) or Cancel: after an upload, to the timeline; otherwise back
// to the tab it came from, with the saved details read back.
export async function leaveDescribe(subject, saved){
  if(subject.kind === 'batch') return openTimeline();
  if(saved && !(await refreshMetadata())) return;
  showPage('timeline');
  if(subject.kind === 'file') return arriveAtTab('files');
  const idx = state.conversations.findIndex((c) => c.id === subject.conversationId);
  arriveAtTab('conversations');
  if(idx >= 0){
    selectConversation(idx);
    address(`#conversations/${idx}`, false);
  }
}

async function refreshMetadata(){
  try{
    const t = await token();
    const [records, uploads] = await Promise.all([fetchConversationRecords(t), fetchUploads(t)]);
    state.records = new Map(records.map((r) => [r.conversation_id, r]));
    state.uploads = uploads;
  } catch(err){
    console.error(err);
    if(expired(err)) toSignIn('signIn.ran_out');
    else setDescribeStatus('describe.load_failed', { detail: err.message, status: errorStatusOf(err), error_kind: errorKindOf(err) });
    return false;
  }
  DEPS.redrawTimeline();
  return true;
}

// --- The timeline and its loading modal (plan §6b) ---

export async function openTimeline(records = null){
  showPage('timeline');
  openLoadingModal();
  setLoadStatus(null);
  showLoadProgress();
  setLoadProgressIndeterminate('progress.preparing');
  try{
    const t = await token();
    const download = PENDING || DEPS.downloadWithBar(t);
    PENDING = null;
    const [{ text, flagHandles }, recs, uploads] = await Promise.all([
      download, records || fetchConversationRecords(t), fetchUploads(t),
    ]);
    if(!DEPS.applyExportText(text, flagHandles, recs, uploads)) throw new Error('your timeline has no conversations');
  } catch(err){
    return loadFailed(err);
  }
  hideLoadProgress();
  closeLoadingModal();
  TIMELINE_LOADED = true;
  arriveAtTimeline();
}

function loadFailed(err){
  console.error(err);
  if(expired(err)){
    closeLoadingModal();
    return toSignIn('signIn.ran_out');
  }
  failLoadProgress();
  setLoadStatus(...DEPS.describeLoadFailure(err));
  offerLoadingModalChoices(() => openTimeline(), () => signOut());
}

// Arriving at the timeline: the view the address names, or the Calendar,
// whose address is written in so Back has somewhere to land.
function arriveAtTimeline(){
  if(TAB_ADDRESS.test(window.location.hash)) return DEPS.applyLocationHash();
  arriveAtTab('calendar');
}

function arriveAtTab(tab){
  switchTab(tab);
  address(`#${tab}`, false);
}

// "Back to timeline" on the Upload page.
export function backToTimeline(){
  if(!TIMELINE_LOADED) return openTimeline();
  showPage('timeline');
  arriveAtTab('calendar');
}

// --- Back and Forward (plan §4) ---

// Every change of address comes here. Describe, and the Upload page while
// files are sending, refuse to be left this way: a page can't stop the
// browser's Back, so it puts its own address back and says why.
export function onAddressChange(){
  const hash = window.location.hash;
  const page = currentPage();
  if(page === 'describe') return holdAddress(describeAddress(SUBJECT), () => setDescribeStatus('describe.back_refused'));
  if(page === 'upload') return uploadAddressChanged(hash);
  if(page === 'timeline') return timelineAddressChanged(hash);
  if(page === 'signIn') address('#signin', false);
}

function holdAddress(own, explain){
  if(window.location.hash === own) return;
  window.history.pushState(null, '', own);
  explain();
}

function uploadAddressChanged(hash){
  if(DEPS.uploading()) return holdAddress('#upload', () => setLoadStatus('load.back_refused'));
  if(TAB_ADDRESS.test(hash) && HAS_DATA) backToTimeline();
}

function timelineAddressChanged(hash){
  if(hash === '#upload') return toUpload(false);
  const subject = subjectFromAddress(hash);
  if(subject) return toDescribe(subject, false);
  DEPS.applyLocationHash();
}
