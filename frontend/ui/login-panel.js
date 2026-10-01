// The load screen's sign-in: the dev login name for local development, or
// Cognito's sign-in for a chosen deployment (migration plan §V2e, E5; see
// infra/deploy-config-file.js for how a deployment is chosen).
//
// A deployment whose settings can't be read, or whose sign-in fails, is
// shown as an error and blocks loading. It never falls back to the dev
// login, which a deployed backend doesn't have.

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
}

async function refresh(){
  const who = await LOGIN.signedInAs();
  if(who) show(true, `Signed in as ${who}.`);
  else show(false, 'Sign in to upload your export.');
}

// Sets up whichever sign-in applies. Resolves once a return from Cognito,
// if this page load is one, has been completed.
export async function initLogin(){
  const name = chosenDeployName();
  if(!name) return;
  document.getElementById('devLoginField').style.display = 'none';
  document.getElementById('cognitoLoginField').style.display = '';
  try{
    const config = await loadDeployConfig(name);
    setApiBase(config.apiBase);
    LOGIN = createCognitoLogin(config);
    useRealLogin({ token: () => LOGIN.accessToken(), label: () => LOGIN.signedInAs() });
    await LOGIN.completeIfReturning();
    await refresh();
  } catch(e){
    console.error(e);
    // Loading must not quietly use something else.
    useRealLogin({ token: async () => { throw e; }, label: async () => null });
    show(false, `Signing in to "${name}" isn't working: ${e.message}`, true);
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
