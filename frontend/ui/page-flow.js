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
import { ensureAuthToken, fetchConversationsPart, usesRealLogin } from '../infra/api-client.js';
import { currentPage, showPage } from './navigation/pages.js';
import { switchTab } from './navigation/tabs.js';
import { selectConversation } from './views/conversations.js';
import { closeLoadingModal, offerLoadingModalChoices, openLoadingModal } from './widgets/loading-modal.js';
import {
  failLoadProgress, hideLoadProgress, setDescribeStatus, setLoadProgressIndeterminate, setLoadStatus,
  setSignInStatus, showLoadProgress,
} from './widgets/status-indicators.js';

// From main.js: signedIn(), accountLabel(), signOut(), refreshSignIn(), resetUploadPage(hasData),
// uploading(), openDescribe(subject), loadTimeline(token) (resolves to false
// when there are no conversations), applyLocationHash(),
// describeLoadFailure(err).
let DEPS = null;
let HAS_DATA = false;
let TIMELINE_LOADED = false;
let SUBJECT = null;
// The timeline's address when the Upload page was opened from it, so going
// back returns to the same tab.
let TIMELINE_ADDRESS = '#calendar';

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
  let first;
  try{
    first = await fetchConversationsPart(await token());
  } catch(err){
    return checkFailed(err);
  }
  HAS_DATA = first.total > 0;
  if(wanted === '#upload' || !HAS_DATA) return toUpload(false);
  await openTimeline();
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
  closeLoadingModal();
  toSignIn(null);
}

// --- Upload ---

export function toUpload(push){
  if(currentPage() === 'timeline' && TAB_ADDRESS.test(window.location.hash)) TIMELINE_ADDRESS = window.location.hash;
  showPage('upload');
  DEPS.resetUploadPage(HAS_DATA);
  address('#upload', push);
}

// The Upload page finished with at least one file processed: Describe
// opens for the files. The timeline is read when Describe is left, since
// describing can move conversations (a changed start and end).
export function afterUpload({ files, processed, failed, stopped }){
  HAS_DATA = true;
  TIMELINE_LOADED = false;
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
// to the tab it came from, with the saved details read back (a changed start
// and end moves a conversation's sessions, so the timeline is read again).
export async function leaveDescribe(subject, saved){
  if(subject.kind === 'batch') return openTimeline();
  if(saved && !(await loadBehindModal())) return;
  showPage('timeline');
  if(subject.kind === 'file') return arriveAtTab('files');
  const idx = state.conversations.findIndex((c) => c.id === subject.conversationId);
  arriveAtTab('conversations');
  if(idx >= 0){
    selectConversation(idx);
    address(`#conversations/${idx}`, false);
  }
}

// --- The timeline and its loading modal (plan §6b) ---

export async function openTimeline(){
  showPage('timeline');
  if(!(await loadBehindModal())) return;
  arriveAtTimeline();
}

// Reads and draws the timeline behind the loading modal (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8b: records,
// then sessions, received in parts, then drawn in turns). Resolves to
// whether it was loaded; a failure is shown in the modal, with Try again.
async function loadBehindModal(){
  openLoadingModal();
  setLoadStatus(null);
  showLoadProgress();
  setLoadProgressIndeterminate('progress.preparing');
  try{
    if(!(await DEPS.loadTimeline(await token()))) throw new Error('your timeline has no conversations');
  } catch(err){
    loadFailed(err);
    return false;
  }
  hideLoadProgress();
  closeLoadingModal();
  TIMELINE_LOADED = true;
  return true;
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
// Back to the tab it was opened from: the address Back landed on, or the
// one remembered when the Upload page opened.
export function backToTimeline(){
  if(!TIMELINE_LOADED) return openTimeline();
  showPage('timeline');
  if(!TAB_ADDRESS.test(window.location.hash)) address(TIMELINE_ADDRESS, false);
  DEPS.applyLocationHash();
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
