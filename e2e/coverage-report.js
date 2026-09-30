// Merges the per-test coverage files coverage.js wrote and prints, per
// script file, which lines and named functions never ran in any test.
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

// url -> { source, covered: Uint8Array per character, functions: Map name -> ran }
const merged = new Map();

for (const file of fs.readdirSync(dir).filter((f) => f.endsWith('.json'))) {
  const entries = JSON.parse(fs.readFileSync(path.join(dir, file), 'utf8'));
  for (const entry of entries) {
    if (!merged.has(entry.url)) {
      merged.set(entry.url, {
        source: entry.source,
        covered: new Uint8Array(entry.source.length),
        functions: new Map(),
      });
    }
    const m = merged.get(entry.url);
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

function lineOffsetInFile(url, source) {
  const rel = decodeURIComponent(new URL(url).pathname).replace(/^\//, '');
  const onDisk = fs.readFileSync(path.join(repoRoot, rel), 'utf8');
  const at = onDisk.indexOf(source);
  if (at < 0) throw new Error(`could not find ${url}'s script text in ${rel}`);
  return { rel, firstLine: onDisk.slice(0, at).split('\n').length - 1 };
}

let totalCode = 0;
let totalRun = 0;
const out = [];

for (const [url, m] of [...merged.entries()].sort()) {
  const { rel, firstLine } = lineOffsetInFile(url, m.source);
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

  out.push(`## ${rel}`);
  out.push(`${code - missed.length} of ${code} code lines ran (${((code - missed.length) / code * 100).toFixed(1)}%).`);
  out.push(`Named functions never called: ${neverCalled.length ? neverCalled.join(', ') : 'none'}.`);
  out.push(`Lines never run: ${spans.length ? spans.map(([a, b]) => (a === b ? `${a}` : `${a}-${b}`)).join(', ') : 'none'}.`);
  out.push('');
}

console.log(`# Coverage: ${totalRun} of ${totalCode} code lines ran (${(totalRun / totalCode * 100).toFixed(1)}%)\n`);
console.log(out.join('\n'));
