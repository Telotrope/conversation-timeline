// The load screen's sign-in: the dev login name for local development, or
// Cognito's sign-in for a chosen deployment (migration plan §V2e, E5; see
// infra/deploy-config-file.js for how a deployment is chosen).
//
// A deployment whose settings can't be read, or whose sign-in fails, is
// shown as an error and blocks loading. It never falls back to the dev
// login, which a deployed backend doesn't have.

import { shownEvent } from '../core/activity-event.js';
import { recordActivity } from '../core/activity-sink.js';
import { setApiBase, useRealLogin } from '../infra/api-client.js';
import { createCognitoLogin } from '../infra/cognito-login.js';
import { chosenDeployName, loadDeployConfig } from '../infra/deploy-config-file.js';

let LOGIN = null;

function show(signedIn, message, isError = false){
  const status = document.getElementById('cognitoLoginStatus');
  status.textContent = message;
  status.style.color = isError ? '#B0392F' : 'var(--ink-faint)';
  document.getElementById('cognitoSignInBtn').style.display = signedIn ? 'none' : '';
  document.getElementById('cognitoSignOutBtn').style.display = signedIn ? '' : 'none';
  // Recorded as signed in or out only: the label itself shows the email
  // address (plan docs/plans/2026-10-02-activity-instrumentation.md §4, C8).
  // A failure's own message goes to the error record.
  recordActivity(shownEvent('signIn', signedIn ? 'signed in' : 'signed out', isError));
  if(isError) recordActivity(shownEvent('error', message, true));
}

async function refresh(){
  const who = await LOGIN.signedInAs();
  if(who) show(true, `Signed in as ${who}.`);
  else show(false, 'Sign in to upload your export.');
}

// Sets up whichever sign-in applies. Resolves once a return from Cognito,
// if this page load is one, has been completed, to { recordActivity }:
// whether the page's activity is recorded (ui/activity-capture.js).
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
  if(name === null) return { recordActivity: true };
  document.getElementById('devLoginField').style.display = 'none';
  document.getElementById('cognitoLoginField').style.display = '';
  try{
    const config = await loadDeployConfig(name);
    setApiBase(config.apiBase);
    LOGIN = createCognitoLogin(config);
    useRealLogin({ token: () => LOGIN.accessToken(), label: () => LOGIN.signedInAs() });
    await LOGIN.completeIfReturning();
    await refresh();
    return { recordActivity: config.recordActivity === true };
  } catch(e){
    console.error(e);
    // Loading must not quietly use something else.
    useRealLogin({ token: async () => { throw e; }, label: async () => null });
    show(false, `Signing in to "${name}" isn't working: ${e.message}`, true);
    return { recordActivity: false };
  }
}

export async function signIn(){
  if(!LOGIN) return; // the error is already shown; see initLogin
  try{
    await LOGIN.signIn();
  } catch(e){
    console.error(e);
    show(false, `Could not start signing in: ${e.message}`, true);
  }
}

export async function signOut(){
  if(!LOGIN) return;
  await LOGIN.signOut();
  await refresh();
}
