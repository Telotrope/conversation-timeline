// Signing in through Cognito's own login page (migration plan §V2e, E5).
//
// The page sends you to Cognito, which handles signing up, confirming your
// email and resetting a password, then sends you back with a one-time code.
// The page trades the code for tokens, proving with PKCE (a standard check
// for pages that hold no secret) that it started the sign-in. The work is
// done by oidc-client-ts (Apache-2.0), loaded as a plain script from
// vendor/oidc-client-ts/ and found here as the global `oidc`.
//
// Tokens are kept in sessionStorage: a reload in the same tab stays signed
// in; closing the tab signs you out of the page. Cognito's access token
// lasts an hour, after which you sign in again.

// config: { cognitoDomain, clientId } from core/deploy-config.js.
export function createCognitoLogin(config){
  const oidc = window.oidc;
  if(!oidc || !oidc.UserManager){
    throw new Error('the sign-in library did not load (vendor/oidc-client-ts/oidc-client-ts.min.js)');
  }
  const domain = config.cognitoDomain;
  const manager = new oidc.UserManager({
    authority: domain,
    client_id: config.clientId,
    // Cognito accepts only the callback addresses the template lists, so
    // this has no query string: whatever the page was opened with is
    // remembered separately (see deploy-config-file.js).
    redirect_uri: window.location.origin + window.location.pathname,
    response_type: 'code',
    scope: 'openid email',
    loadUserInfo: false,
    automaticSilentRenew: false,
    // Given directly rather than discovered: Cognito's discovery document
    // lives at the user pool's address, not the login domain's.
    metadata: {
      issuer: domain,
      authorization_endpoint: `${domain}/oauth2/authorize`,
      token_endpoint: `${domain}/oauth2/token`,
    },
  });

  return {
    // If this page load is the return from Cognito, trades the code for
    // tokens, then removes the code from the address bar either way.
    async completeIfReturning(){
      const params = new URLSearchParams(window.location.search);
      if(!params.has('state') || !(params.has('code') || params.has('error'))) return;
      try{
        await manager.signinRedirectCallback();
      } finally {
        for(const p of ['code', 'state', 'error', 'error_description']) params.delete(p);
        const query = params.toString();
        window.history.replaceState(null, '', window.location.pathname + (query ? `?${query}` : '') + window.location.hash);
      }
    },
    // The current access token, or null when signed out or expired.
    async accessToken(){
      const user = await manager.getUser();
      return user && !user.expired ? user.access_token : null;
    },
    // Who is signed in (their email), or null.
    async signedInAs(){
      const user = await manager.getUser();
      return user && !user.expired ? (user.profile.email || user.profile.sub) : null;
    },
    // coverage-exempt-start: runs just before the page leaves for Cognito's sign-in (docs/plans/completed/2026-10-02-browser-coverage-every-test.md §3)
    signIn(){ return manager.signinRedirect(); },
    // coverage-exempt-end
    // Forgets the tokens in this tab. Cognito's own session stays, so the
    // next sign-in may not ask for the password again.
    signOut(){ return manager.removeUser(); },
  };
}
