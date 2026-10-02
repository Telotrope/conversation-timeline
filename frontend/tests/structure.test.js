// Structural rules for the page's modules (docs/plans/completed/2026-09-30-split-timeline-script.md,
// section 1 and step V7): no circular imports, no file over 1,000 lines, and
// every import pointing down the layers.
//
// Cycle detection is a depth-first search over the import graph read below,
// rather than madge as the plan named: madge depends, through precinct, on
// postcss-values-parser, which is MPL-2.0, outside this project's
// permissive-license rule.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const MAX_LINES = 1000;

function moduleFiles(dir = ROOT) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) return ['tests', 'node_modules'].includes(e.name) ? [] : moduleFiles(p);
    return e.name.endsWith('.js') ? [p] : [];
  });
}

const rel = (p) => path.relative(ROOT, p).split(path.sep).join('/');
const files = moduleFiles().map(rel).sort();

// Every import in these modules is a single-line static `import ... from '...'`.
// A dynamic import() would slip past this graph, so it is refused outright.
function importsOf(file) {
  const src = fs.readFileSync(path.join(ROOT, file), 'utf8');
  assert.doesNotMatch(src, /\bimport\s*\(/, `${file} uses a dynamic import()`);
  return [...src.matchAll(/^import .* from '(.+)';$/gm)]
    .map((m) => rel(path.resolve(path.dirname(path.join(ROOT, file)), m[1])));
}

const graph = new Map(files.map((f) => [f, importsOf(f)]));

// Which folder a file belongs to, for the layer rules.
function layer(file) {
  if (file === 'main.js') return 'main';
  const parts = file.split('/');
  if (parts[0] === 'ui') return parts.length === 2 ? 'ui' : `ui/${parts[1]}`;
  return parts[0];
}

// For each layer, the layers its files may import. Imports within a layer
// are checked separately below.
const MAY_IMPORT = {
  core: [],
  infra: ['core'],
  'ui/render': ['core'],
  'ui/widgets': ['core'],
  'ui/navigation': ['core'],
  'ui/views': ['core', 'infra', 'ui/render', 'ui/widgets', 'ui/navigation'],
  ui: ['core', 'infra', 'ui/render', 'ui/widgets', 'ui/navigation', 'ui/views'],
  main: ['core', 'infra', 'ui/render', 'ui/widgets', 'ui/navigation', 'ui/views', 'ui'],
};

// Imports allowed between files of the same layer, other than core/ and the
// three lowest ui/ folders, which may import freely within themselves.
const SAME_LAYER = {
  'ui/views': new Set([
    'ui/views/calendar.js -> ui/views/review.js',
    'ui/views/conversations.js -> ui/views/review.js',
    'ui/views/analytics.js -> ui/views/review.js',
  ]),
  ui: new Set([
    'ui/flag-edits.js -> ui/refresh-views.js',
    'ui/load-flow.js -> ui/router.js',
  ]),
};

test('the modules import only existing files', () => {
  for (const [file, deps] of graph) {
    for (const dep of deps) assert.ok(graph.has(dep), `${file} imports missing ${dep}`);
  }
});

test('no module imports itself, directly or through a chain', () => {
  const state = new Map(); // file -> 'visiting' | 'done'
  const cycles = [];
  function visit(file, trail) {
    if (state.get(file) === 'done') return;
    if (state.get(file) === 'visiting') {
      cycles.push([...trail.slice(trail.indexOf(file)), file].join(' -> '));
      return;
    }
    state.set(file, 'visiting');
    for (const dep of graph.get(file) || []) visit(dep, [...trail, file]);
    state.set(file, 'done');
  }
  for (const file of graph.keys()) visit(file, []);
  assert.deepEqual(cycles, []);
});

test('no module is over 1,000 lines', () => {
  for (const file of files) {
    const lines = fs.readFileSync(path.join(ROOT, file), 'utf8').split('\n').length;
    assert.ok(lines <= MAX_LINES, `${file} has ${lines} lines`);
  }
});

test('every import points down the layers', () => {
  const wrong = [];
  for (const [file, deps] of graph) {
    const from = layer(file);
    for (const dep of deps) {
      const to = layer(dep);
      if (to === from) {
        const free = ['core', 'ui/render', 'ui/widgets', 'ui/navigation'].includes(from);
        if (!free && !(SAME_LAYER[from] && SAME_LAYER[from].has(`${file} -> ${dep}`))) {
          wrong.push(`${file} -> ${dep}`);
        }
      } else if (!MAY_IMPORT[from].includes(to)) {
        wrong.push(`${file} -> ${dep}`);
      }
    }
  }
  assert.deepEqual(wrong, []);
});

// Comments are stripped first: several explain, in words, what the code
// deliberately does not do.
function code(file) {
  return fs.readFileSync(path.join(ROOT, file), 'utf8')
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '')
    .replace(/\s\/\/.*$/gm, '');
}

test('core/ touches no browser, network or storage API', () => {
  for (const file of files.filter((f) => layer(f) === 'core')) {
    assert.doesNotMatch(code(file), /\b(document|window|fetch|localStorage|requestAnimationFrame)\b/, file);
  }
});

test('infra/ never touches the page', () => {
  for (const file of files.filter((f) => layer(f) === 'infra')) {
    assert.doesNotMatch(code(file), /\bdocument\b/, file);
  }
});
