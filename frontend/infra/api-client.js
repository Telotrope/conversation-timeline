// Everything that talks to the timeline backend: working out its address,
// getting a development login token, uploading with progress, reading the
// answers the server gives in parts (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §8c), saving your
// flag corrections, and turning failed responses into readable messages.

import { joinParts, readAllParts } from '../core/parts.js';
import { resolveUrl } from '../core/server-url.js';
import { S3_FILE_ROUTE, S3_UPLOAD_ROUTE, requestEvent, routeTemplate } from '../core/activity-event.js';
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
// docs/plans/completed/2026-10-02-activity-instrumentation.md §1). Never sent to S3.
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
// lines). facts: extra fields for this route's record ({part} for
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
// The failure as a PageError: 'data_integrity' when the server says its
// stored data can't be read (plan
// docs/plans/2026-10-06-load-only-what-the-page-shows.md §12.4), otherwise
// 'server_error'.
export async function requestFailure(what, res){
  const { text, kind } = await readFailure(what, res);
  return new PageError(text, kind, res.status);
}

export async function describeFailure(what, res){
  return (await readFailure(what, res)).text;
}

async function readFailure(what, res){
  try{
    const body = await res.text();
    try{
      const parsed = JSON.parse(body);
      if(parsed && typeof parsed.error === 'string'){
        const kind = parsed.error_kind === 'data_integrity' ? 'data_integrity' : 'server_error';
        return { text: `${what} failed (${res.status}): ${parsed.error}`, kind };
      }
    } catch(e){ /* not JSON -- fall through to raw text below */ }
    return { text: `${what} failed (${res.status})${body ? ': ' + body : ''}`, kind: 'server_error' };
  } catch(e){
    return { text: `${what} failed (${res.status}), and the error response itself couldn't be read: ${e.message}`, kind: 'server_error' };
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
// registerAbort(fn), when given, receives a function that cancels the send
// (the Upload page's Stop); a cancelled send rejects like a cut-off one.
export function putWithProgress(url, body, onProgress, registerAbort = () => {}){
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
    registerAbort(() => xhr.abort());
    xhr.send(body);
  });
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

// Persists your confirmed flags to the backend, resolving to
// { outcome: SaveOutcome, reply?, detail?, status?, errorKind? }. `msg` is a
// message of Review's page on screen (core/review-rows.js): its
// conversation, its id and the handle GET /messages issued for it, which
// proves to the server the message is real (migration plan §V2c).
// setRowOverrides already updates state.overrides and re-renders
// optimistically before calling this; a failed PATCH is reported in the
// outcome, not silently swallowed, but doesn't roll back the optimistic
// local update. A saved one's `reply` is the server's answer: the
// message's flags, its session counted again, and the raised data version.
export async function patchFlagsToBackend(msg, values){
  if(!AUTH_TOKEN){
    return { outcome: SaveOutcome.NOT_LOGGED_IN };
  }
  if(!msg.conversationId || !msg.messageId || !msg.handle){
    return { outcome: SaveOutcome.NO_SERVER_ID };
  }
  try{
    const res = await apiFetch(`/conversations/${encodeURIComponent(msg.conversationId)}/messages/${encodeURIComponent(msg.messageId)}/flags`, {
      method: 'PATCH',
      token: AUTH_TOKEN,
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...values, handle: msg.handle }),
    });
    if(res.status === 403) return { outcome: SaveOutcome.STALE_PAGE, status: 403 };
    if(!res.ok) throw new PageError(`server returned ${res.status}`, 'server_error', res.status);
    return { outcome: SaveOutcome.SAVED, reply: await res.json() };
  } catch(e){
    return {
      outcome: SaveOutcome.SERVER_ERROR, detail: e.message,
      status: e instanceof PageError ? e.status : null, errorKind: errorKindOf(e),
    };
  }
}

// --- Reading (plan docs/plans/2026-10-06-load-only-what-the-page-shows.md §5, §8c) ---

async function jsonOrFailure(what, res){
  if(!res.ok) throw await requestFailure(what, res);
  return res.json();
}

// `path` with `params` as its query string; null and undefined values are
// left out.
export function withQuery(path, params){
  const query = new URLSearchParams();
  for(const [key, value] of Object.entries(params)){
    if(value !== null && value !== undefined) query.set(key, String(value));
  }
  const text = query.toString();
  return text ? `${path}?${text}` : path;
}

async function getJson(what, token, path){
  return jsonOrFailure(what, await apiFetch(path, { token }));
}

// One part of GET /conversations: { conversations, total, cursor,
// data_version }.
export function fetchConversationsPart(token, cursor = null){
  return getJson('reading your conversations', token, withQuery('/conversations', { cursor }));
}

// One part of GET /sessions: { sessions, total, cursor, data_version }.
export function fetchSessionsPart(token, cursor = null){
  return getJson('reading your sessions', token, withQuery('/sessions', { cursor }));
}

// GET /conversations, every part: every conversation's record, metadata
// included. Empty when the user has uploaded nothing, which is how the page
// decides between the Upload page and the timeline. `on`: readAllParts's
// onPart and onRestart (core/parts.js).
export async function fetchConversationRecords(token, on = {}){
  const { parts } = await readAllParts((cursor) => fetchConversationsPart(token, cursor), on);
  return joinParts(parts, 'conversations');
}

// GET /uploads, every part: the user's files, for the Files tab and the
// Describe page. Each part is sorted newest first; the whole list is sorted
// again here.
export async function fetchUploads(token, on = {}){
  const { parts } = await readAllParts(
    (cursor) => getJson('reading your files', token, withQuery('/uploads', { cursor })), on);
  return joinParts(parts, 'uploads').sort((a, b) => Date.parse(b.uploaded_at) - Date.parse(a.uploaded_at));
}

// One part of GET /messages, Review's rows; `params` are its query
// (core/review-query.js).
export function fetchMessagesPart(token, params){
  return getJson('reading your messages', token, withQuery('/messages', params));
}

// One part of GET /conversations/{id}/files: the files presented in one
// conversation.
export function fetchConversationFilesPart(token, conversationId, cursor = null){
  return getJson('reading the conversation\'s files', token,
    withQuery(`/conversations/${encodeURIComponent(conversationId)}/files`, { cursor }));
}

// GET /files/{conversation}/{message}/{number}: a short-lived address for a
// stored file, with its name and kind.
export function fetchFileAddress(token, conversationId, messageId, number){
  const path = `/files/${encodeURIComponent(conversationId)}/${encodeURIComponent(messageId)}/${encodeURIComponent(number)}`;
  return getJson('opening the file', token, path);
}

// A stored file's text, from the address fetchFileAddress gave. Not
// through apiFetch: on AWS the address is a signed S3 link, which gets no
// session header or token. Recorded as "s3 GET files/…", never the address.
export async function downloadFileText(url){
  const started = Date.now();
  noteRequestStarted();
  let res = null;
  try{
    res = await fetch(serverUrl(url));
    const text = res.ok ? await res.text() : null;
    recordActivity(requestEvent({
      t: started, method: 'GET', route: S3_FILE_ROUTE, status: res.status, ms: Date.now() - started, requestId: null,
    }));
    if(text === null) throw await requestFailure('downloading the file', res);
    return text;
  } catch(e){
    if(e instanceof PageError) throw e;
    recordActivity(requestEvent({
      t: started, method: 'GET', route: S3_FILE_ROUTE, status: res ? res.status : null,
      ms: Date.now() - started, requestId: null, errorKind: errorKindOf(e),
    }));
    throw e;
  } finally {
    noteRequestFinished();
  }
}

// GET /analyses/{name}: one of the two analyses the server computes
// ('trend' or 'time-of-day'), done or still working.
export function fetchAnalysis(token, name, params){
  return getJson('computing the analysis', token, withQuery(`/analyses/${name}`, params));
}

// POST /detect: one part of the scan, from `cursor` (null to start).
// `part` counts the requests of this scan, for the activity log.
export async function postDetect(token, cursor, part){
  const res = await apiFetch('/detect', {
    method: 'POST',
    token,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(cursor === null ? {} : { cursor }),
    facts: { part },
  });
  return jsonOrFailure('scanning your messages', res);
}

// One part of GET /export, the annotated download.
export function fetchExportPart(token, cursor = null){
  return getJson('building your annotated download', token, withQuery('/export', { cursor }));
}

function putJson(path, token, body){
  return apiFetch(path, {
    method: 'PUT',
    token,
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
}

// PUT /uploads/{id}/metadata: an edit for every conversation that first
// came in that file, sent again with each part's cursor until the server
// has rewritten them all (§8c). Not started over when the data version
// changes: the edit itself changes the records, and repeating it is
// harmless. onPart({ done, total }) after each part. Resolves to the
// changed records.
export async function saveFileMetadata(token, uploadId, edit, onPart = () => {}){
  const records = [];
  let cursor = null;
  do{
    const part = await jsonOrFailure('saving the file\'s details',
      await putJson(`/uploads/${encodeURIComponent(uploadId)}/metadata`, token, cursor === null ? edit : { ...edit, cursor }));
    records.push(...part.conversations);
    onPart(part);
    cursor = part.cursor ?? null;
  } while(cursor !== null);
  return records;
}

// PUT /conversations/{id}/metadata: an edit for one conversation. Resolves
// to its changed record.
export async function saveConversationMetadata(token, conversationId, edit){
  return jsonOrFailure('saving the conversation\'s details',
    await putJson(`/conversations/${encodeURIComponent(conversationId)}/metadata`, token, edit));
}
