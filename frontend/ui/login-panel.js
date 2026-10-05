// The load screen's sign-in: the dev login name for local development, or
// Cognito's sign-in for a chosen deployment (migration plan §V2e, E5; see
// infra/deploy-config-file.js for how a deployment is chosen).
//
// A deployment whose settings can't be read, or whose sign-in fails, is
// shown as an error and blocks loading. It never falls back to the dev
// login, which a deployed backend doesn't have.

import { shownEvent } from '../core/activity-event.js';
import { recordActivity } from '../core/activity-sink.js';
import { errorKindOf } from '../core/page-error.js';
import { setApiBase, useRealLogin } from '../infra/api-client.js';
import { createCognitoLogin } from '../infra/cognito-login.js';
import { chosenDeployName, loadDeployConfig } from '../infra/deploy-config-file.js';
import { pageMessage, recordedValues } from './widgets/page-messages.js';

let LOGIN = null;

// id: the sign-in line's message (ui/widgets/page-messages.js), shown with
// its values and recorded for the activity log by identifier only: the
// line can show the email address (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, C8 and C17).
function show(signedIn, id, values = {}){
  const entry = pageMessage(id);
  const status = document.getElementById('cognitoLoginStatus');
  status.textContent = entry.text(values);
  status.style.color = entry.isError ? '#B0392F' : 'var(--ink-faint)';
  document.getElementById('cognitoSignInBtn').style.display = signedIn ? 'none' : '';
  document.getElementById('cognitoSignOutBtn').style.display = signedIn ? '' : 'none';
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
  document.getElementById('devLoginField').style.display = 'none';
  document.getElementById('cognitoLoginField').style.display = '';
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

// coverage-exempt-start: runs just before the page leaves for Cognito's sign-in, and Chrome discards a page's coverage when it leaves (docs/plans/2026-10-02-browser-coverage-every-test.md §3)
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
