// Merges the per-test coverage files fixtures.js wrote and prints, per
// script file, which lines and named functions never ran in any test.
// Every program file under frontend/ is listed, including any no test
// loaded (shown as never loaded, 0%). Scripts reported without their text,
// and scripts that aren't repository files (a page a test made up), are
// named at the top and left out
// (docs/plans/2026-10-02-browser-coverage-every-test.md).
//
// Usage: node coverage-report.js <COVERAGE_DIR> <repo root>
//
// A line counts as run if the code at its first non-blank character ran.
// Blank lines and comment-only lines are not counted. Line numbers are
// positions in the file on disk: for the inline <script> in timeline.html
// that means offsetting by where the script starts inside the HTML.

const fs = require('fs');
const path = require('path');

const [dir, repoRoot] = process.argv.slice(2);
if (!dir || !repoRoot) {
  console.error('usage: node coverage-report.js <COVERAGE_DIR> <repo root>');
  process.exit(2);
}

// repository path -> { source, covered: Uint8Array per character, functions: Map name -> ran }
const merged = new Map();
const withoutText = [];
const notRepositoryFiles = new Set();

// The repository file a script URL names, by path: the tests' servers all
// serve the repository's own layout, whatever their address and query
// string. null for a path that isn't a file here.
function repositoryPath(url) {
  const rel = decodeURIComponent(new URL(url).pathname).replace(/^\//, '');
  const full = path.join(repoRoot, rel);
  return rel && fs.existsSync(full) && fs.statSync(full).isFile() ? rel : null;
}

for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.json'))) {
  const { test: testName, entries } = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
  for (const entry of entries) {
    if (typeof entry.source !== 'string') {
      withoutText.push(`${entry.url} (in ${testName})`);
      continue;
    }
    const key = repositoryPath(entry.url);
    if (!key) {
      notRepositoryFiles.add(new URL(entry.url).pathname);
      continue;
    }
    if (!merged.has(key)) {
      merged.set(key, {
        source: entry.source,
        covered: new Uint8Array(entry.source.length),
        functions: new Map(),
      });
    }
    const m = merged.get(key);
    if (m.source !== entry.source) {
      throw new Error(`${entry.url} changed between tests; rerun on a stable tree`);
    }

    // V8 nests block ranges inside their function's range. Sorting by start,
    // then longest first, and letting later ranges overwrite earlier ones
    // leaves each character with its innermost range's count.
    const counts = new Int32Array(entry.source.length).fill(-1);
    const ranges = entry.functions.flatMap((f) => f.ranges);
    ranges.sort((a, b) => a.startOffset - b.startOffset || b.endOffset - a.endOffset);
    for (const r of ranges) counts.fill(r.count, r.startOffset, r.endOffset);
    counts.forEach((c, i) => { if (c > 0) m.covered[i] = 1; });

    for (const f of entry.functions) {
      if (!f.functionName) continue;
      const key = `${f.functionName}@${f.ranges[0].startOffset}`;
      m.functions.set(key, (m.functions.get(key) || false) || f.ranges[0].count > 0);
    }
  }
}

function lineOffsetInFile(rel, source) {
  const onDisk = fs.readFileSync(path.join(repoRoot, rel), 'utf8');
  const at = onDisk.indexOf(source);
  if (at < 0) throw new Error(`could not find the script text recorded for ${rel} in the file`);
  return { rel, firstLine: onDisk.slice(0, at).split('\n').length - 1 };
}

// Every program file under frontend/, tests and installed packages aside.
function programFiles(dirRel) {
  return fs.readdirSync(path.join(repoRoot, dirRel), { withFileTypes: true }).flatMap((d) => {
    const rel = path.posix.join(dirRel, d.name);
    if (d.isDirectory()) return ['tests', 'node_modules'].includes(d.name) ? [] : programFiles(rel);
    return d.name.endsWith('.js') ? [rel] : [];
  });
}

// Files no test loaded: their whole text, nothing run.
const neverLoaded = new Set();
for (const rel of programFiles('frontend')) {
  if (merged.has(rel)) continue;
  const source = fs.readFileSync(path.join(repoRoot, rel), 'utf8');
  merged.set(rel, { source, covered: new Uint8Array(source.length), functions: new Map() });
  neverLoaded.add(rel);
}

let totalCode = 0;
let totalRun = 0;
const out = [];

for (const [key, m] of [...merged.entries()].sort()) {
  const { rel, firstLine } = lineOffsetInFile(key, m.source);
  const lines = m.source.split('\n');
  let offset = 0;
  let code = 0;
  const missed = [];
  let inBlockComment = false;
  lines.forEach((text, i) => {
    const trimmed = text.trim();
    const firstChar = offset + text.length - text.trimStart().length;
    offset += text.length + 1;
    if (inBlockComment) { if (trimmed.includes('*/')) inBlockComment = false; return; }
    if (trimmed.startsWith('/*')) { if (!trimmed.includes('*/')) inBlockComment = true; return; }
    if (!trimmed || trimmed.startsWith('//')) return;
    code += 1;
    if (!m.covered[firstChar]) missed.push(firstLine + i + 1);
  });

  const neverCalled = [...m.functions.entries()]
    .filter(([, ran]) => !ran)
    .map(([key]) => {
      const [name, start] = key.split('@');
      const line = firstLine + m.source.slice(0, Number(start)).split('\n').length;
      return `${name} (line ${line})`;
    });

  totalCode += code;
  totalRun += code - missed.length;

  // Collapse consecutive line numbers into ranges for reading.
  const spans = [];
  for (const n of missed) {
    const last = spans[spans.length - 1];
    if (last && last[1] === n - 1) last[1] = n; else spans.push([n, n]);
  }

  out.push(`## ${rel}${neverLoaded.has(rel) ? ' -- never loaded by any test' : ''}`);
  out.push(`${code - missed.length} of ${code} code lines ran (${((code - missed.length) / code * 100).toFixed(1)}%).`);
  out.push(`Named functions never called: ${neverCalled.length ? neverCalled.join(', ') : 'none'}.`);
  out.push(`Lines never run: ${spans.length ? spans.map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`)).join(', ') : 'none'}.`);
  out.push('');
}

console.log(`# Coverage: ${totalRun} of ${totalCode} code lines ran (${(totalRun / totalCode * 100).toFixed(1)}%)\n`);
console.log(`Program files under frontend/ never loaded by any test: ${neverLoaded.size ? [...neverLoaded].join(', ') : 'none'}.`);
console.log(`Scripts recorded without their text (left out): ${withoutText.length ? withoutText.join('; ') : 'none'}.`);
console.log(`Scripts that are not repository files (left out): ${notRepositoryFiles.size ? [...notRepositoryFiles].join(', ') : 'none'}.\n`);
console.log(out.join('\n'));
