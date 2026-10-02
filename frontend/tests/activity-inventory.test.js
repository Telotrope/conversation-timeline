// The handler inventory (plan docs/plans/2026-10-02-activity-instrumentation.md
// §4 and §8): every kind of event the page's code listens for must be one the
// activity recorder covers, so the recorder can't silently fall behind the
// page. Reads every `addEventListener('<kind>'` and `.on<kind> =` in the
// page's modules (tests and node_modules left out).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

// What the recorder does with each kind of user action the page responds
// to. `input` (a keystroke in a search box) is covered by recording only
// what is submitted: Enter in a text box (keydown) or a form's submit.
const COVERED = {
  click: 'recorded',
  change: 'recorded',
  input: 'recorded only on Enter (keydown) or submit',
  hashchange: 'recorded as a view',
  submit: 'recorded',
  keydown: 'Enter in a text box recorded as a submit',
  pagehide: 'sends what waits',
  visibilitychange: 'sends what waits when hidden',
};

// Events of an XMLHttpRequest the page itself made (its upload), not
// actions of the user: allowed only on a receiver named xhr.
const REQUEST_EVENTS = new Set(['progress', 'load', 'error', 'abort']);

function moduleFiles(dir = ROOT){
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if(e.isDirectory()) return ['tests', 'node_modules'].includes(e.name) ? [] : moduleFiles(p);
    return e.name.endsWith('.js') ? [p] : [];
  });
}

// Comments are stripped first: several mention handlers in words.
function code(file){
  return fs.readFileSync(file, 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '')
    .replace(/\s\/\/.*$/gm, '');
}

function inventory(){
  const found = [];
  const problems = [];
  for(const file of moduleFiles()){
    const rel = path.relative(ROOT, file);
    const src = code(file);
    for(const m of src.matchAll(/addEventListener\(\s*(['"]?)([^'",\s)]+)\1/g)){
      if(!m[1]) problems.push(`${rel}: addEventListener with a kind that isn't written out (${m[2]})`);
      else found.push({ rel, kind: m[2] });
    }
    for(const m of src.matchAll(/([\w$.()'"[\]-]*)\.on([a-z]+)\s*=(?!=)/g)){
      if(REQUEST_EVENTS.has(m[2]) && /^xhr\b/.test(m[1])) continue;
      found.push({ rel, kind: m[2], property: true });
    }
  }
  return { found, problems };
}

test('every kind of event the page listens for is covered by the activity recorder', () => {
  const { found, problems } = inventory();
  assert.deepEqual(problems, []);
  const uncovered = found.filter((f) => !(f.kind in COVERED)).map((f) => `${f.rel}: ${f.kind}`);
  assert.deepEqual(uncovered, [], 'the page listens for a kind of event the recorder does not record');
  // The scan finds what it should: the kinds the plan's inventory listed.
  const kinds = new Set(found.map((f) => f.kind));
  for(const kind of ['click', 'change', 'input', 'hashchange']) assert.ok(kinds.has(kind), kind);
  // ...including a handler set as a property (the restored-session notice's
  // Dismiss button, `.onclick = `), and not the upload's own request events.
  assert.ok(found.some((f) => f.property && f.kind === 'click'), 'an .onclick handler');
  assert.ok(!found.some((f) => REQUEST_EVENTS.has(f.kind)), 'request events left out');
});

test("the recorder's own listeners cover every kind but input", () => {
  const src = code(path.join(ROOT, 'ui', 'activity-listeners.js'));
  const own = new Set([...src.matchAll(/addEventListener\('(\w+)'/g)].map((m) => m[1]));
  assert.deepEqual([...own].sort(), Object.keys(COVERED).filter((k) => k !== 'input').sort());
});
