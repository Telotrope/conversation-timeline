// Everything that talks to the timeline backend: working out its address,
// getting a development login token, uploading and downloading with
// progress, saving your flag corrections, and turning failed responses into
// readable messages.

import { state } from '../core/state.js';

// Not a relative path: timeline.html isn't served by timeline-api and is
// opened separately, so relative fetch()es would resolve against the
// wrong origin. See the migration plan's V2a.
//
// Defaults to same-machine local dev (see the root README's "Running this
// locally"), where the browser's own 127.0.0.1 reaches the backend
// directly. That default is wrong whenever the backend runs on a
// different machine than the browser -- a remote/cloud dev environment
// reached through a port-forwarding proxy, for example -- so it can be
// overridden once via a `?api_base=<url>` query parameter; the override
// is remembered in localStorage so it doesn't need to be retyped on every
// reload. No trailing slash: every call site appends a leading-slash
// path directly onto this value.
function resolveApiBase(){
  const fromQuery = new URLSearchParams(window.location.search).get('api_base');
  if(fromQuery){
    const trimmed = fromQuery.replace(/\/+$/, '');
    localStorage.setItem('timeline_api_base', trimmed);
    return trimmed;
  }
  return localStorage.getItem('timeline_api_base') || 'http://127.0.0.1:3000';
}

export const API_BASE = resolveApiBase();

let AUTH_TOKEN = null;

// Forgets the login, so the next request signs in again.
export function clearAuthToken(){
  AUTH_TOKEN = null;
}

// Dev-only: mints a bearer token from timeline-api's checked-in throwaway
// keypair, since there's no real Cognito pool to log in against locally.
// Never a real authentication mechanism -- see timeline-api's dev_only
// module and the migration plan's V2a.
export async function ensureAuthToken(sub){
  if(AUTH_TOKEN) return AUTH_TOKEN;
  if(!sub) throw new Error('enter a dev login name first');
  const res = await fetch(`${API_BASE}/_dev/login`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ sub }),
  });
  if(!res.ok) throw new Error(await describeFailure('dev login', res));
  const body = await res.json();
  AUTH_TOKEN = body.token;
  // Remembered so a reload can offer to pick the session back up without
  // making you choose the file again -- see tryRestoreSession.
  try{ localStorage.setItem('timeline_dev_sub', sub); } catch(e){ /* private mode; restore just won't offer */ }
  return AUTH_TOKEN;
}

// A bare HTTP status code ("upload failed (413)") doesn't explain what
// actually went wrong -- both axum's own built-in error responses (e.g.
// "Failed to buffer the request body: length limit exceeded" for a
// too-large upload) and this app's own JSON error bodies ({"error": "..."})
// carry real, useful text. Read whichever shape came back rather than
// discarding it. Reading the body can itself fail (already consumed,
// network cut mid-read); that failure is folded into the message too,
// not silently dropped.
export async function describeFailure(what, res){
  try{
    const text = await res.text();
    try{
      const parsed = JSON.parse(text);
      if(parsed && typeof parsed.error === 'string') return `${what} failed (${res.status}): ${parsed.error}`;
    } catch(e){ /* not JSON -- fall through to raw text below */ }
    return `${what} failed (${res.status})${text ? ': ' + text : ''}`;
  } catch(e){
    return `${what} failed (${res.status}), and the error response itself couldn't be read: ${e.message}`;
  }
}

// fetch() cannot report upload progress -- it has no equivalent of
// xhr.upload.onprogress and its streaming request bodies aren't portable --
// so the one request that carries the file body uses XMLHttpRequest. Every
// other call in the load path stays on fetch.
export function putWithProgress(url, body, onProgress){
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('PUT', url);
    xhr.upload.onprogress = (e) => {
      if(e.lengthComputable) onProgress(e.loaded, e.total);
    };
    xhr.onload = () => resolve({ ok: xhr.status >= 200 && xhr.status < 300, status: xhr.status, text: xhr.responseText });
    // Both fire for genuine transport failures; reject with something the
    // caller's TypeError hint can still recognize as "couldn't reach it".
    xhr.onerror = () => reject(new TypeError('Failed to fetch'));
    xhr.onabort = () => reject(new TypeError('upload aborted'));
    xhr.send(body);
  });
}

// Reads a response body chunk by chunk so a large download reports real
// progress. Content-Length is absent often enough (chunked encoding, proxies)
// that the no-total case shows transferred bytes instead of a made-up
// percentage.
export async function readBodyWithProgress(res, onProgress){
  const total = Number(res.headers.get('Content-Length')) || 0;
  if(!res.body || !res.body.getReader) return res.text();
  const reader = res.body.getReader();
  const chunks = [];
  let loaded = 0;
  for(;;){
    const { done, value } = await reader.read();
    if(done) break;
    chunks.push(value);
    loaded += value.length;
    onProgress(loaded, total);
  }
  return new TextDecoder().decode(await new Blob(chunks).arrayBuffer());
}

// What happened when your confirmed flags were sent to the backend. The
// wording shown for each is the interface's choice, in ui/flag-edits.js.
export const SaveOutcome = Object.freeze({
  SAVED: 'saved',
  NOT_LOGGED_IN: 'not-logged-in',
  NO_SERVER_ID: 'no-server-id',   // the message has no server-side id to save under
  SERVER_ERROR: 'server-error',   // `detail` says what went wrong
});

// Persists your confirmed flags to the real backend, resolving to
// { outcome: SaveOutcome, detail? }. setRowOverrides already updates
// state.overrides and re-renders optimistically before calling this; a failed
// PATCH is reported in the outcome, not silently swallowed, but doesn't roll
// back the optimistic local update. See the migration plan's
// V2a: this only persists for as long as the in-memory local-dev backend
// stays running -- there is no database behind it yet.
export async function patchFlagsToBackend(msg, values){
  if(!AUTH_TOKEN){
    return { outcome: SaveOutcome.NOT_LOGGED_IN };
  }
  const conv = state.rawData && state.rawData[msg.conv];
  const rawMsg = conv && conv.chat_messages && conv.chat_messages[msg.rawIndex];
  if(!conv || !rawMsg){
    return { outcome: SaveOutcome.NO_SERVER_ID };
  }
  try{
    const res = await fetch(`${API_BASE}/conversations/${conv.uuid}/messages/${rawMsg.uuid}/flags`, {
      method: 'PATCH',
      headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${AUTH_TOKEN}` },
      body: JSON.stringify(values),
    });
    if(!res.ok) throw new Error(`server returned ${res.status}`);
    return { outcome: SaveOutcome.SAVED };
  } catch(e){
    return { outcome: SaveOutcome.SERVER_ERROR, detail: e.message };
  }
}
