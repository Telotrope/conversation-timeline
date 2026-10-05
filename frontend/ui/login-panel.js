// The Sign-in page: the dev login name for local development, or Cognito's
// sign-in for a chosen deployment (migration plan §V2e, E5; see
// infra/deploy-config-file.js for how a deployment is chosen). Also tells
// the page flow whether you are signed in, and who as (plan
// docs/plans/2026-10-05-screen-flow.md §5).
//
// A deployment whose settings can't be read, or whose sign-in fails, is
// shown as an error and blocks loading. It never falls back to the dev
// login, which a deployed backend doesn't have.

import { shownEvent } from '../core/activity-event.js';
import { recordActivity } from '../core/activity-sink.js';
import { errorKindOf } from '../core/page-error.js';
import { clearAuthToken, ensureAuthToken, setApiBase, signedInLabel, useRealLogin, usesRealLogin } from '../infra/api-client.js';
import { createCognitoLogin } from '../infra/cognito-login.js';
import { chosenDeployName, loadDeployConfig } from '../infra/deploy-config-file.js';
import { pageMessage, recordedValues } from './widgets/page-messages.js';

let LOGIN = null;
// The sign-in line's message now, so a refresh can tell a stale "Signed in
// as…" from an error it must leave in place.
let SHOWN = null;

// id: the sign-in line's message (ui/widgets/page-messages.js), shown with
// its values and recorded for the activity log by identifier only: the
// line can show the email address (plan
// docs/plans/completed/2026-10-02-activity-instrumentation.md §4, C8 and C17).
function show(signedIn, id, values = {}){
  const entry = pageMessage(id);
  SHOWN = id;
  const status = document.getElementById('cognitoLoginStatus');
  status.textContent = entry.text(values);
  status.classList.toggle('is-error', entry.isError);
  document.getElementById('cognitoSignInBtn').hidden = signedIn;
  document.getElementById('cognitoSignOutBtn').hidden = !signedIn;
  recordActivity(shownEvent('signIn', id, entry.isError, recordedValues(entry, values)));
}

async function refresh(){
  const who = await LOGIN.signedInAs();
  if(who) show(true, 'signIn.signed_in', { who });
  else show(false, 'signIn.signed_out');
}

// Sets up whichever sign-in applies. Resolves once a return from Cognito,
// if this page load is one, has been completed, to { recordActivity,
// pageVersion }: whether the page's activity is recorded
// (ui/activity-capture.js), and the version its records carry ('local' for
// local development, 'unknown' when a deployment's settings don't say).
//
// Local development (no deployment chosen) records, and sends to the local
// backend, which logs the records to its output. This departs from the plan's
// §4, which says local development records to the browser console only; its
// §8 browser test needs the local backend to receive POST /activity, and the
// two can't both hold. A deployment records only when its settings say
// recordActivity: true, and a deployment whose settings can't be read
// records nothing.
export async function initLogin(){
  const tag = document.querySelector('meta[name="timeline-deploy"]');
  const name = chosenDeployName(tag ? tag.getAttribute('content') ?? '' : null);
  if(name === null) return { recordActivity: true, pageVersion: 'local' };
  document.getElementById('devLoginField').hidden = true;
  document.getElementById('cognitoLoginField').hidden = false;
  try{
    const config = await loadDeployConfig(name);
    setApiBase(config.apiBase);
    LOGIN = createCognitoLogin(config);
    useRealLogin({ token: () => LOGIN.accessToken(), label: () => LOGIN.signedInAs() });
    await LOGIN.completeIfReturning();
    await refresh();
    return { recordActivity: config.recordActivity === true, pageVersion: config.pageVersion || 'unknown' };
  } catch(e){
    console.error(e);
    // Loading must not quietly use something else.
    useRealLogin({ token: async () => { throw e; }, label: async () => null });
    show(false, 'signIn.failed', { name, detail: e.message, error_kind: errorKindOf(e) });
    return { recordActivity: false, pageVersion: 'unknown' };
  }
}

// coverage-exempt-start: runs just before the page leaves for Cognito's sign-in, and Chrome discards a page's coverage when it leaves (docs/plans/completed/2026-10-02-browser-coverage-every-test.md §3)
export async function signIn(){
  if(!LOGIN) return; // the error is already shown; see initLogin
  try{
    await LOGIN.signIn();
    // coverage-exempt-end
  } catch(e){
    console.error(e);
    show(false, 'signIn.start_failed', { detail: e.message, error_kind: errorKindOf(e) });
  }
}

export async function signOut(){
  if(!LOGIN) return;
  await LOGIN.signOut();
  await refresh();
}

// For the page flow's return to Sign-in: a line still saying "Signed in
// as…" is redrawn from the sign-in as it is now, since it may have run
// out. Any other line (an error, "Sign in to…") is left as it is.
export async function refreshSignInLine(){
  if(LOGIN && SHOWN === 'signIn.signed_in') await refresh();
}

// The name a dev login was last made under, remembered by ensureAuthToken
// (infra/api-client.js) so the next visit is still signed in.
const DEV_NAME = 'timeline_dev_sub';

function rememberedDevName(){
  try{ return localStorage.getItem(DEV_NAME); } catch(e){
    console.info('Could not read the remembered dev login name:', e.message);
    return null;
  }
}

// Whether you are signed in: with Cognito, a sign-in that hasn't run out;
// locally, a dev login name remembered from an earlier visit, which is put
// back in its field.
export async function isSignedIn(){
  if(usesRealLogin()) return Boolean(await signedInLabel());
  const name = rememberedDevName();
  if(!name) return false;
  document.getElementById('devLoginSub').value = name;
  return true;
}

// Who is signed in, for the account line.
export async function accountLabel(){
  if(usesRealLogin()) return (await signedInLabel()) || '';
  return document.getElementById('devLoginSub').value.trim();
}

// Signs out in either mode. Locally that forgets the remembered name, so
// the next visit asks again.
export async function signOutEverywhere(){
  clearAuthToken();
  if(usesRealLogin()) return signOut();
  try{ localStorage.removeItem(DEV_NAME); } catch(e){
    console.info('Could not forget the dev login name:', e.message);
  }
}

// Local development's Continue: logs in under the typed name, which
// ensureAuthToken remembers. A failure (no name, server unreachable) is
// thrown for the caller to show.
export async function devContinue(){
  clearAuthToken();
  await ensureAuthToken(document.getElementById('devLoginSub').value.trim());
}
