// The page's message catalog (ui/widgets/page-messages.js): the activity log
// records these identifiers instead of the wording (plan
// docs/plans/2026-10-02-activity-instrumentation.md §4, C17).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PAGE_MESSAGES, pageMessage, recordedValues } from '../ui/widgets/page-messages.js';
import { ERROR_KINDS } from '../core/page-error.js';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

function moduleFiles(dir = ROOT){
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if(e.isDirectory()) return ['tests', 'node_modules'].includes(e.name) ? [] : moduleFiles(p);
    return e.name.endsWith('.js') ? [p] : [];
  });
}

const ID = /^[a-z][A-Za-z]*(?:\.[a-z_]+)?$/;

test('every identifier is a plain dotted name with wording and an error flag', () => {
  for(const [id, entry] of Object.entries(PAGE_MESSAGES)){
    assert.match(id, ID, id);
    assert.equal(typeof entry.isError, 'boolean', id);
    assert.ok(entry.text === null || typeof entry.text === 'function', id);
    for(const key of entry.record || []) assert.ok(['count', 'attempt', 'max_attempts', 'status', 'error_kind'].includes(key), `${id}: ${key}`);
  }
});

test('the wording each message shows', () => {
  const answer = { status: 'processing', attempt: 2, max_attempts: 3, last_error: 'boom' };
  const expected = {
    'load.choose_file': [{}, 'Choose a conversations.json file first.'],
    'load.failed': [{ detail: 'x failed (500)', hint: '' }, 'Could not load that file through the backend — x failed (500)'],
    'flags.loaded': [{ count: 1 }, 'Loaded 1 of your confirmed flag from the server.'],
    'save.saved': [{}, 'Saved.'],
    'save.server_error': [{ detail: 'server returned 500' }, 'Could not save to the server: server returned 500'],
    restored: [{ sub: 'alice' }, 'Picked up where you left off — the export you last loaded as "alice". Use "Load a different file…" to start fresh.'],
    'signIn.signed_in': [{ who: 'a@b.c' }, 'Signed in as a@b.c.'],
    'signIn.failed': [{ name: 'dev', detail: 'nope' }, 'Signing in to "dev" isn\'t working: nope'],
    'signIn.start_failed': [{ detail: 'nope' }, 'Could not start signing in: nope'],
    'wait.retrying': [{ answer, elapsedMs: 1000 }, 'The server hit an error (boom) and is trying again automatically: attempt 2 of 3. AWS waits 1–2 minutes between attempts. — 1s'],
  };
  for(const [id, [values, text]] of Object.entries(expected)) assert.equal(pageMessage(id).text(values), text, id);
  assert.equal(pageMessage('flags.loaded').text({ count: 3 }), 'Loaded 3 of your confirmed flags from the server.');
  assert.equal(pageMessage('progress.failed').text, null);
  // Every other message has fixed wording.
  for(const [id, entry] of Object.entries(PAGE_MESSAGES)){
    if(entry.text && !(id in expected) && !id.startsWith('wait.')) assert.ok(entry.text({}).length > 0, id);
  }
});

test('only the listed live values are recorded, never wording', () => {
  const entry = pageMessage('load.failed');
  assert.deepEqual(recordedValues(entry, { detail: 'secret', hint: ' Is it running?', status: 500, error_kind: 'server_error' }),
    { status: 500, error_kind: 'server_error' });
  assert.deepEqual(recordedValues(entry, { detail: 'x', status: null }), {});
  assert.equal(recordedValues(pageMessage('save.saved'), { detail: 'x' }), undefined);
  assert.equal(recordedValues(entry, undefined), undefined);
});

test('an unknown identifier is refused, naming it', () => {
  assert.throws(() => pageMessage('load.nope'), /no page message named "load.nope"/);
  assert.throws(() => pageMessage('toString'), /no page message named "toString"/);
});

test("every identifier the page's code names is in the catalog, and every one in the catalog is used", () => {
  const named = new Set();
  for(const file of moduleFiles()){
    if(file.endsWith(path.join('widgets', 'page-messages.js'))) continue;
    const src = fs.readFileSync(file, 'utf8');
    for(const m of src.matchAll(/'((?:load|progress|wait|save|flags|export|signIn)\.[a-z_]+|restored)'/g)) named.add(m[1]);
  }
  const catalog = new Set(Object.keys(PAGE_MESSAGES));
  assert.deepEqual([...named].filter((id) => !catalog.has(id)), [], 'named in the code but not in the catalog');
  assert.deepEqual([...catalog].filter((id) => !named.has(id)).sort(), [], 'in the catalog but never named');
});

test('error kinds are plain names', () => {
  for(const kind of ERROR_KINDS) assert.match(kind, /^[a-z_]+$/);
});
