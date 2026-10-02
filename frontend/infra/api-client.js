// Everything that talks to the timeline backend: working out its address,
// getting a development login token, uploading and downloading with
// progress, saving your flag corrections, and turning failed responses into
// readable messages.

import { state } from '../core/state.js';
import { resolveUrl } from '../core/server-url.js';
import { S3_EXPORT_ROUTE, S3_UPLOAD_ROUTE, requestEvent, routeTemplate } from '../core/activity-event.js';
import { noteRequestFinished, noteRequestStarted, recordActivity } from '../core/activity-sink.js';
import { PageError, errorKindOf } from '../core/page-error.js';

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

// `let`, not `const`: a chosen deployment replaces it with the deployed
// API's address (see ui/login-panel.js). Importers see the change, since
// module exports are live.
export let API_BASE = resolveApiBase();

export function setApiBase(url){
  API_BASE = url;
}

// An address the backend handed back, made fetchable; see core/server-url.js.
export function serverUrl(url){
  return resolveUrl(API_BASE, url);
}

// One id per page load, sent with every request to our API as
// x-timeline-session and kept with every activity record, so the page's
// records and the server's log lines of one visit can be put together (plan
// docs/plans/2026-10-02-activity-instrumentation.md §1). Never sent to S3.
//
// crypto.randomUUID exists only on secure pages (https, or this machine);
// elsewhere a version-4 UUID is built from crypto.getRandomValues, which
// every page has.
function newSessionId(){
  if(typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  const b = crypto.getRandomValues(new Uint8Array(16));
  b[6] = (b[6] & 0x0f) | 0x40;
  b[8] = (b[8] & 0x3f) | 0x80;
  const hex = Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export const SESSION_ID = newSessionId();
const SESSION_HEADER = 'x-timeline-session';

// The answer's declared size in bytes, or undefined when it doesn't say.
function contentLength(res){
  const raw = res.headers.get('Content-Length');
  if(raw === null) return undefined;
  const n = Number(raw);
  return Number.isInteger(n) && n >= 0 ? n : undefined;
}

// Every request to our API goes through here: it adds the sign-in token (when
// given) and the session id, tells the activity recorder a request is in
// flight, and records the request (route with ids replaced by {id}, status,
// time, and API Gateway's request id, which joins it to the server's log
// lines). facts: extra fields for this route's record ({offset, limit} for
// /detect, {scan} for /uploads). Resolves to fetch's Response; a request
// that gets no answer is recorded with its error and the error is thrown on.
export async function apiFetch(path, { method = 'GET', token = null, headers = {}, body, facts } = {}){
  const sent = { ...headers, [SESSION_HEADER]: SESSION_ID };
  if(token) sent['Authorization'] = `Bearer ${token}`;
  const route = routeTemplate(path);
  const started = Date.now();
  noteRequestStarted();
  try{
    const res = await fetch(`${API_BASE}${path}`, { method, headers: sent, body });
    recordActivity(requestEvent({
      t: started, method, route, status: res.status, ms: Date.now() - started,
      bytes: contentLength(res), requestId: res.headers.get('apigw-requestid'), facts,
    }));
    return res;
  } catch(e){
    recordActivity(requestEvent({
      t: started, method, route, status: null, ms: Date.now() - started, requestId: null, errorKind: errorKindOf(e), facts,
    }));
    throw e;
  } finally {
    noteRequestFinished();
  }
}

// Downloads the processed export from the signed link GET /export handed
// back, reporting progress (see readBodyWithProgress). Not through apiFetch:
// a signed S3 link gets no session header or token. Recorded as
// "s3 GET export/…", the whole download counted as one request, body
// included. Resolves to { res, text }, text null when the answer wasn't OK
// (the body is then left unread, for describeFailure).
export async function downloadSignedExport(url, onProgress){
  const started = Date.now();
  noteRequestStarted();
  let res = null;
  let received;
  try{
    res = await fetch(url);
    const text = res.ok
      ? await readBodyWithProgress(res, (loaded, total) => { received = loaded; onProgress(loaded, total); })
      : null;
    recordActivity(requestEvent({
      t: started, method: 'GET', route: S3_EXPORT_ROUTE, status: res.status, ms: Date.now() - started,
      bytes: received, requestId: null,
    }));
    return { res, text };
  } catch(e){
    recordActivity(requestEvent({
      t: started, method: 'GET', route: S3_EXPORT_ROUTE, status: res ? res.status : null,
      ms: Date.now() - started, requestId: null, errorKind: errorKindOf(e),
    }));
    throw e;
  } finally {
    noteRequestFinished();
  }
}

// Sends one batch of activity records (infra/activity-recorder.js). Not
// through apiFetch: sending records must not itself be recorded, nor count
// as the page being busy. keepalive lets it finish after the page closes.
export async function postActivityBatch(body, { token, keepalive }){
  const res = await fetch(`${API_BASE}/activity`, {
    method: 'POST',
    keepalive,
    headers: {
      'Content-Type': 'application/json',
      'Authorization': `Bearer ${token}`,
      [SESSION_HEADER]: SESSION_ID,
    },
    body,
  });
  return { ok: res.ok, status: res.status };
}

// GET /uploads/{id}: whether the backend has finished processing an upload.
// Resolves to the route's JSON; throws with the server's message otherwise.
export async function fetchUploadStatus(token, uploadId){
  const res = await apiFetch(`/uploads/${encodeURIComponent(uploadId)}`, { token });
  if(!res.ok) throw await requestFailure('checking on the upload', res);
  return res.json();
}

let AUTH_TOKEN = null;

// The real sign-in in use (Cognito, see infra/cognito-login.js), or null
// for the dev login: { token, label }, async functions resolving to the
// current access token and to who is signed in, each null when signed out.
let REAL_LOGIN = null;

export function useRealLogin(login){
  REAL_LOGIN = login;
}

export function usesRealLogin(){
  return REAL_LOGIN !== null;
}

// Who is signed in with the real sign-in, or null.
export function signedInLabel(){
  return REAL_LOGIN ? REAL_LOGIN.label() : Promise.resolve(null);
}

// The token of the last sign-in, without asking for a new one, or null:
// what the activity recorder sends its batches with.
export function lastAuthToken(){
  return AUTH_TOKEN;
}

// Forgets the login, so the next request signs in again.
export function clearAuthToken(){
  AUTH_TOKEN = null;
}

// Dev-only: mints a bearer token from timeline-api's checked-in throwaway
// keypair, since there's no real Cognito pool to log in against locally.
// Never a real authentication mechanism -- see timeline-api's dev_only
// module and the migration plan's V2a.
//
// With a real sign-in in use, asks it instead, every time, so an expired
// token is never reused; `sub` is ignored.
export async function ensureAuthToken(sub){
  if(REAL_LOGIN){
    AUTH_TOKEN = await REAL_LOGIN.token();
    if(!AUTH_TOKEN) throw new PageError('sign in first, with the Sign in button above', 'not_logged_in');
    return AUTH_TOKEN;
  }
  if(AUTH_TOKEN) return AUTH_TOKEN;
  if(!sub) throw new PageError('enter a dev login name first', 'not_logged_in');
  const res = await apiFetch('/_dev/login', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ sub }),
  });
  if(!res.ok) throw await requestFailure('dev login', res);
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
// A failed answer as an error to throw: describeFailure's message, with the
// HTTP status and the kind 'server_error' for the activity log.
export async function requestFailure(what, res){
  return new PageError(await describeFailure(what, res), 'server_error', res.status);
}

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
//
// The url is a signed S3 link, so no session header or token goes with it.
// Recorded as "s3 PUT raw/…" (the key and signature are never recorded),
// with the bytes sent.
export function putWithProgress(url, body, onProgress){
  const started = Date.now();
  let sentBytes;
  const finish = (status, errorKind) => {
    noteRequestFinished();
    recordActivity(requestEvent({
      t: started, method: 'PUT', route: S3_UPLOAD_ROUTE, status, ms: Date.now() - started,
      bytes: sentBytes, requestId: null, errorKind,
    }));
  };
  noteRequestStarted();
  return new Promise((resolve, reject) => {
    const xhr = new XMLHttpRequest();
    xhr.open('PUT', url);
    xhr.upload.onprogress = (e) => {
      if(e.lengthComputable){
        sentBytes = e.loaded;
        onProgress(e.loaded, e.total);
      }
    };
    xhr.onload = () => {
      finish(xhr.status);
      resolve({ ok: xhr.status >= 200 && xhr.status < 300, status: xhr.status, text: xhr.responseText });
    };
    // Both fire for genuine transport failures; reject with something the
    // caller's TypeError hint can still recognize as "couldn't reach it".
    xhr.onerror = () => { finish(null, 'network'); reject(new TypeError('Failed to fetch')); };
    xhr.onabort = () => { finish(null, 'aborted'); reject(new TypeError('upload aborted')); };
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
  SERVER_ERROR: 'server-error',   // `detail` says what went wrong; `status` and `errorKind` its kind
  STALE_PAGE: 'stale-page',       // the server refused the message's handle: reload the page's data
});

// Persists your confirmed flags to the real backend, resolving to
// { outcome: SaveOutcome, detail?, status?, errorKind? }. setRowOverrides already updates
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
  // Every save carries the handle GET /export issued for this message,
  // proving to the server the message is real (migration plan §V2c).
  const handle = rawMsg && state.flagHandles && state.flagHandles[rawMsg.uuid];
  if(!conv || !rawMsg || !handle){
    return { outcome: SaveOutcome.NO_SERVER_ID };
  }
  try{
    const res = await apiFetch(`/conversations/${conv.uuid}/messages/${rawMsg.uuid}/flags`, {
      method: 'PATCH',
      token: AUTH_TOKEN,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...values, handle }),
    });
    if(res.status === 403) return { outcome: SaveOutcome.STALE_PAGE, status: 403 };
    if(!res.ok) throw new PageError(`server returned ${res.status}`, 'server_error', res.status);
    return { outcome: SaveOutcome.SAVED };
  } catch(e){
    return {
      outcome: SaveOutcome.SERVER_ERROR, detail: e.message,
      status: e instanceof PageError ? e.status : null, errorKind: errorKindOf(e),
    };
  }
}
