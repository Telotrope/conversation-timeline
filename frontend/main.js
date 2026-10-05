// The page's starting point: connects every button, box and switch in the
// markup to the code that handles it, hands each page module the functions
// of the others it needs (modules at the same level may not import each
// other), then opens the right page (ui/page-flow.js). Nothing else lives
// here.

import { buildBlocks } from './core/blocks.js';
import { attachFlags } from './core/flags.js';
import { state } from './core/state.js';
import { decideActivityRecording, startActivityCapture } from './ui/activity-capture.js';
import { installActivityListeners } from './ui/activity-listeners.js';
import { exportAnnotatedConversations } from './ui/annotated-export.js';
import { connectDescribe, describeCancel, describeDone, openDescribe } from './ui/describe-form.js';
import { approveRow, onVisibilityToggleChanged, setRowOverrides } from './ui/flag-edits.js';
import { applyExportText, chooseFiles, connectUploadPage, describeLoadFailure, downloadWithBar, handleUploadClick, redrawTimeline, resetUploadPage, stopUpload, uploading } from './ui/load-flow.js';
import { accountLabel, devContinue, initLogin, isSignedIn, refreshSignInLine, signIn, signOutEverywhere } from './ui/login-panel.js';
import { rememberLocation } from './ui/navigation/location.js';
import { switchTab } from './ui/navigation/tabs.js';
import * as flow from './ui/page-flow.js';
import { applyLocationHash } from './ui/router.js';
import { runAnalysis } from './ui/views/analytics.js';
import { renderConvList, setConversationEditHandler } from './ui/views/conversations.js';
import { setFileEditHandler } from './ui/views/files.js';
import { renderReviewTable, setFlagEditHandlers, showFirstReviewPage } from './ui/views/review.js';
import { setSignInStatus } from './ui/widgets/status-indicators.js';
import { errorKindOf, errorStatusOf } from './core/page-error.js';

// First, so the activity log (ui/activity-capture.js) sees everything from
// the start; whether it is kept and sent is decided once the sign-in is set
// up, below.
installActivityListeners({ win: window, doc: document, ...startActivityCapture() });

flow.connectPageFlow({
  signedIn: isSignedIn, accountLabel, signOut: signOutEverywhere, refreshSignIn: refreshSignInLine,
  resetUploadPage, uploading, openDescribe,
  downloadWithBar, applyExportText, redrawTimeline, applyLocationHash, describeLoadFailure,
});
connectUploadPage({
  hasData: flow.hasData, dataArrived: flow.dataArrived, toSignIn: flow.toSignIn, afterUpload: flow.afterUpload,
});
connectDescribe({
  done: (subject) => flow.leaveDescribe(subject, true),
  cancel: (subject) => flow.leaveDescribe(subject, false),
  ranOut: () => flow.toSignIn('signIn.ran_out'),
});
setFileEditHandler((uploadId) => flow.toDescribe({ kind: 'file', uploadId }));
setConversationEditHandler((conversationId) => flow.toDescribe({ kind: 'conversation', conversationId }));

// Sign-in page.
document.getElementById('cognitoSignInBtn').addEventListener('click', signIn);
document.getElementById('cognitoSignOutBtn').addEventListener('click', () => flow.signOut());
document.getElementById('accountSignOutBtn').addEventListener('click', () => flow.signOut());
document.getElementById('checkAgainBtn').addEventListener('click', () => flow.enterSignedIn());
document.getElementById('devLoginBtn').addEventListener('click', async () => {
  try{
    await devContinue();
  } catch(err){
    console.error(err);
    return setSignInStatus('signIn.check_failed', { detail: err.message, status: errorStatusOf(err), error_kind: errorKindOf(err) });
  }
  setSignInStatus(null);
  await flow.enterSignedIn();
});

// Upload page.
document.getElementById('loadConvFile').addEventListener('change', chooseFiles);
document.getElementById('loadBtn').addEventListener('click', handleUploadClick);
document.getElementById('stopBtn').addEventListener('click', stopUpload);
document.getElementById('uploadBackBtn').addEventListener('click', () => flow.backToTimeline());

// Describe page.
document.getElementById('describeSaveBtn').addEventListener('click', describeDone);
document.getElementById('describeLeaveBtn').addEventListener('click', describeCancel);

// Timeline. Nothing renders until an export is downloaded (applyExportText
// in ui/load-flow.js), which fills state.conversations and the rest and
// draws every view itself.
document.getElementById('addConversationsBtn').addEventListener('click', () => flow.toUpload(true));
state.blocks = buildBlocks();
attachFlags();

window.addEventListener('hashchange', flow.onAddressChange);
document.querySelectorAll('nav.tabs button').forEach(b=>{
  b.addEventListener('click', ()=>{ switchTab(b.dataset.tab); rememberLocation(); });
});

document.getElementById('convSearch').addEventListener('input', (e)=> renderConvList(e.target.value));

document.getElementById('reviewSearch').addEventListener('input', showFirstReviewPage);
document.getElementById('reviewFilter').addEventListener('change', showFirstReviewPage);
setFlagEditHandlers(setRowOverrides, approveRow);
document.getElementById('exportAnnotatedBtn').addEventListener('click', exportAnnotatedConversations);
document.getElementById('toggleShowAuto').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowUser').addEventListener('change', onVisibilityToggleChanged);
document.getElementById('toggleShowReplies').addEventListener('change', ()=>{
  state.showReplies = document.getElementById('toggleShowReplies').checked;
  renderReviewTable();
});

document.querySelectorAll('.analytics-item').forEach(btn=>{
  btn.addEventListener('click', ()=> runAnalysis(btn.dataset.analysis, {}));
});

// Last thing in the file, so everything it calls already exists. Opening a
// page waits for the sign-in to be set up (instant without a chosen
// deployment), since it needs to know who you are.
initLogin().then(({ recordActivity, pageVersion }) => {
  decideActivityRecording(recordActivity, pageVersion);
  return flow.start();
});
