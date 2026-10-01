// A pretend Cognito login domain for the browser tests: just enough of
// Cognito's sign-in to check the page's side of it (migration plan §V2e,
// E5). Real Cognito is checked only by a deployment (check D3).
//
// GET /oauth2/authorize skips the login form and sends the browser straight
// back with a one-time code. POST /oauth2/token checks the PKCE proof, the
// code, the client and the return address, then answers with tokens. The
// access token comes from the local backend's POST /_dev/login, so the local
// backend accepts it; the ID token is unsigned, which the page's library
// doesn't check (it reads the claims only).

const http = require('http');
const crypto = require('crypto');

const base64url = (buf) => Buffer.from(buf).toString('base64url');

function unsignedJwt(claims) {
  return `${base64url(JSON.stringify({ alg: 'none', typ: 'JWT' }))}.${base64url(JSON.stringify(claims))}.`;
}

// clientId: the only client accepted. apiBase: the local backend.
// email/sub: who "signs in". Resolves to { url, breakNextProof, close }.
async function startCognitoStandIn({ clientId, apiBase, sub, email }) {
  const codes = new Map();
  let breakNext = false;
  let url = null;

  const cors = (req) => ({
    'Access-Control-Allow-Origin': req.headers.origin || '*',
    'Access-Control-Allow-Headers': 'content-type, authorization',
    'Access-Control-Allow-Methods': 'POST, OPTIONS',
  });
  const send = (res, status, headers, body) => { res.writeHead(status, headers); res.end(body); };
  const refuse = (req, res, error) => send(res, 400,
    { ...cors(req), 'Content-Type': 'application/json' }, JSON.stringify({ error }));

  async function token(req, res, body) {
    const form = new URLSearchParams(body);
    const entry = codes.get(form.get('code'));
    codes.delete(form.get('code'));
    if (form.get('grant_type') !== 'authorization_code' || !entry) return refuse(req, res, 'invalid_grant');
    if (form.get('client_id') !== clientId) return refuse(req, res, 'invalid_client');
    if (form.get('redirect_uri') !== entry.redirectUri) return refuse(req, res, 'invalid_grant');
    const proof = base64url(crypto.createHash('sha256').update(form.get('code_verifier') || '').digest());
    if (proof !== entry.challenge) return refuse(req, res, 'invalid_grant');

    const login = await fetch(`${apiBase}/_dev/login`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ sub }),
    });
    if (!login.ok) throw new Error(`stand-in could not get a token from the backend: ${login.status}`);
    const { token: accessToken } = await login.json();
    const now = Math.floor(Date.now() / 1000);
    const idToken = unsignedJwt({
      sub, email, aud: clientId, iss: url, iat: now, exp: now + 3600,
      ...(entry.nonce ? { nonce: entry.nonce } : {}),
    });
    send(res, 200, { ...cors(req), 'Content-Type': 'application/json' }, JSON.stringify({
      access_token: accessToken, id_token: idToken, token_type: 'Bearer', expires_in: 3600,
    }));
  }

  const server = http.createServer((req, res) => {
    const here = new URL(req.url, url);
    if (req.method === 'GET' && here.pathname === '/oauth2/authorize') {
      const q = here.searchParams;
      if (q.get('client_id') !== clientId || q.get('response_type') !== 'code'
        || q.get('code_challenge_method') !== 'S256' || !q.get('code_challenge') || !q.get('state')) {
        return send(res, 400, { 'Content-Type': 'text/plain' }, `bad authorize request: ${here.search}`);
      }
      const code = crypto.randomUUID();
      codes.set(code, {
        // A deliberately wrong expected proof, when a test asks for one.
        challenge: breakNext ? `${q.get('code_challenge')}-wrong` : q.get('code_challenge'),
        redirectUri: q.get('redirect_uri'),
        nonce: q.get('nonce'),
      });
      breakNext = false;
      const back = new URL(q.get('redirect_uri'));
      back.searchParams.set('code', code);
      back.searchParams.set('state', q.get('state'));
      return send(res, 302, { Location: back.href }, '');
    }
    if (here.pathname === '/oauth2/token' && req.method === 'OPTIONS') return send(res, 204, cors(req), '');
    if (here.pathname === '/oauth2/token' && req.method === 'POST') {
      let body = '';
      req.on('data', (d) => { body += d; });
      req.on('end', () => token(req, res, body).catch((e) => send(res, 500, cors(req), String(e))));
      return undefined;
    }
    return send(res, 404, { 'Content-Type': 'text/plain' }, 'not part of the stand-in');
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  url = `http://127.0.0.1:${server.address().port}`;
  return {
    url,
    breakNextProof: () => { breakNext = true; },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}

module.exports = { startCognitoStandIn };
