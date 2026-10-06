// Measures every wait of plan docs/plans/2026-10-06-load-only-what-the-page-shows.md
// §9 in a real browser, and §8b's longest gap between moves of the progress
// bar during each: preparing, sending, processing and scanning an upload;
// opening the timeline; a page of Review, a search with no other filter,
// and Claude's replies; the five analyses, the two from the server twice
// (first, then saved); and the annotated download.
//
// A "move" is a change of a bar's fill width, or of its words once the
// clocks in them (" · N s so far", and the processing wait's " — 1m 5s")
// are taken off: a clock alone is not progress (§8b). A wait's longest gap is the longest time between its
// start, each move, and its end.
//
// Not a test: it prints what it measured as JSON and checks nothing.
//
// Usage (from e2e/):
//   node measure-waits.js --page <timeline.html address> --file <export> [--sub <name>]
//     [--out <file.json>]
// Signs in with local development's sign-in (the address must point the page
// at a local timeline-api with ?api_base=...).

const fs = require('fs');
const { chromium } = require('@playwright/test');

function args() {
  const out = {};
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 2) out[argv[i].replace(/^--/, '')] = argv[i + 1];
  if (!out.page || !out.file) throw new Error('usage: node measure-waits.js --page <address> --file <export> [--sub <name>] [--out <file>]');
  return out;
}

// Installed before the page's own scripts: every move of every bar, by the
// bar's fill id, with the time it happened.
function recordMoves() {
  window.__moves = [];
  const last = new Map();
  // Both clocks off: the bar's own (" · 12 s so far") and the one the
  // processing wait writes into its words (" — 12s", " — 1m 5s").
  const words = (label) => (label ? label.textContent
    .replace(/ · \d+ s so far$/, '')
    .replace(/ — (\d+h )?(\d+m )?\d+[sm]( ·|$)/, '$3') : '');
  const look = () => {
    for (const fill of document.querySelectorAll('.progress-fill')) {
      const label = fill.closest('.progress-track')?.nextElementSibling;
      const now = `${fill.style.width}|${fill.className}|${words(label)}`;
      if (last.get(fill.id) !== now) {
        last.set(fill.id, now);
        window.__moves.push({ t: Date.now(), bar: fill.id, width: fill.style.width, words: words(label) });
      }
    }
  };
  new MutationObserver(look).observe(document, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['style', 'class'] });
}

const waits = [];

// Runs `act`, then waits for `done`; records the wait from the start of
// `act` to `done` resolving, with its longest gap between bar moves.
async function measure(page, name, act, done) {
  const start = Date.now();
  await act();
  await done();
  const end = Date.now();
  const moves = (await page.evaluate(() => window.__moves)).filter((m) => m.t >= start && m.t <= end);
  const times = [start, ...moves.map((m) => m.t), end];
  let gap = 0;
  for (let i = 1; i < times.length; i++) gap = Math.max(gap, times[i] - times[i - 1]);
  const wait = { name, ms: end - start, longest_gap_ms: gap, moves: moves.length };
  waits.push(wait);
  console.error(`${name}: ${(wait.ms / 1000).toFixed(2)} s, longest gap ${(gap / 1000).toFixed(2)} s, ${moves.length} moves`);
  return wait;
}

// Review's count is final once it no longer says "at least".
async function reviewSettled(page) {
  await page.waitForFunction(() => {
    const count = document.getElementById('reviewCount')?.textContent || '';
    const busy = !document.getElementById('reviewProgress')?.hidden && !/Could not/.test(document.getElementById('reviewProgressLabel')?.textContent || '');
    return /^\d+ messages?$/.test(count) && !busy;
  }, null, { timeout: 600_000, polling: 50 });
}

async function analysisDrawn(page) {
  await page.waitForFunction(() => !document.querySelector('#analyticsMain .progress-wrap'), null, { timeout: 600_000, polling: 50 });
}

// The upload's load-status words mark where one wait ends and the next
// begins; the waits are split at the first move showing each.
function splitUpload(moves, start, end) {
  const status = (re) => moves.find((m) => m.t >= start && re.test(m.words));
  const marks = [
    ['preparing the file', start],
    ['sending the file', status(/^Sending your file/)?.t],
    ['processing', status(/^(Waiting for the server|Processing)/)?.t],
    ['scanning', status(/^(Starting the scan|Scanning your messages)/)?.t],
  ].filter(([, t]) => t !== undefined);
  return marks.map(([name, t], i) => {
    const until = i + 1 < marks.length ? marks[i + 1][1] : end;
    const inside = moves.filter((m) => m.t >= t && m.t <= until).map((m) => m.t);
    const times = [t, ...inside, until];
    let gap = 0;
    for (let j = 1; j < times.length; j++) gap = Math.max(gap, times[j] - times[j - 1]);
    return { name, ms: until - t, longest_gap_ms: gap, moves: inside.length };
  });
}

async function main() {
  const opts = args();
  const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
  const page = await browser.newPage({ acceptDownloads: true });
  await page.addInitScript(recordMoves);
  page.on('pageerror', (err) => console.error(`page error: ${err}`));
  try {
    await page.goto(opts.page);
    await page.waitForSelector('#signInPage:visible, #uploadPage:visible', { timeout: 60_000 });
    if (await page.locator('#signInPage').isVisible()) {
      await page.fill('#devLoginSub', opts.sub || `measure-${Date.now()}`);
      await page.click('#devLoginBtn');
    }
    await page.waitForSelector('#uploadPage:visible', { timeout: 60_000 });

    // The upload, scan ticked, to the Describe page.
    await page.setInputFiles('#loadConvFile', opts.file);
    await page.check('#autoDetectCheckbox');
    const uploadStart = Date.now();
    await page.click('#loadBtn');
    await page.waitForSelector('#describePage:visible', { timeout: 1_800_000 });
    const uploadEnd = Date.now();
    const moves = await page.evaluate(() => window.__moves);
    for (const wait of splitUpload(moves, uploadStart, uploadEnd)) {
      waits.push(wait);
      console.error(`${wait.name}: ${(wait.ms / 1000).toFixed(2)} s, longest gap ${(wait.longest_gap_ms / 1000).toFixed(2)} s, ${wait.moves} moves`);
    }
    waits.push({ name: 'upload to Describe, in all', ms: uploadEnd - uploadStart });

    await page.waitForSelector('#describeBody .describe-section, #describeBody .hint', { timeout: 60_000 });
    // Done saves the details, then reads the timeline behind the loading
    // modal: over once the sessions have arrived and the modal has closed.
    await measure(page, 'opening the timeline', async () => {
      const sessions = page.waitForResponse((r) => new URL(r.url()).pathname === '/sessions', { timeout: 600_000 });
      await page.click('#describeSaveBtn');
      await sessions;
    }, () => page.waitForSelector('#loadingModal', { state: 'hidden', timeout: 600_000 }));

    await measure(page, 'a page of Review (first rows)', () => page.click('button[data-tab="review"]'),
      () => page.waitForSelector('#reviewTable tbody tr[data-msg-id]', { timeout: 600_000 }));
    await measure(page, 'a page of Review (count final)', async () => {}, () => reviewSettled(page));

    // The page asks once typing pauses for 300 ms, so this includes that
    // pause; the first rows are those of the search's first answer.
    await measure(page, 'a search with no other filter (first rows)', async () => {
      const searched = page.waitForResponse((r) => r.url().includes('/messages') && r.url().includes('search=the'), { timeout: 600_000 });
      await page.fill('#reviewSearch', 'the');
      await searched;
    }, () => page.waitForSelector('#reviewTable tbody tr[data-msg-id]', { timeout: 600_000 }));
    await measure(page, 'a search with no other filter (count final)', async () => {}, () => reviewSettled(page));
    await page.fill('#reviewSearch', '');
    await reviewSettled(page);

    await measure(page, "Claude's replies on", () => page.check('#toggleShowReplies'),
      () => page.waitForSelector('#reviewTable .claude-reply-row', { timeout: 600_000 }));
    await page.uncheck('#toggleShowReplies');

    await page.click('button[data-tab="analytics"]');
    for (const name of ['friction', 'length', 'idlegap', 'trend', 'timeofday']) {
      await measure(page, `analysis ${name}`, () => page.click(`.analytics-item[data-analysis="${name}"]`), () => analysisDrawn(page));
    }
    for (const name of ['trend', 'timeofday']) {
      await page.click('.analytics-item[data-analysis="friction"]');
      await analysisDrawn(page);
      await measure(page, `analysis ${name}, saved`, () => page.click(`.analytics-item[data-analysis="${name}"]`), () => analysisDrawn(page));
    }

    await page.click('button[data-tab="review"]');
    let download = null;
    await measure(page, 'the annotated download', async () => {
      const waiting = page.waitForEvent('download', { timeout: 1_800_000 });
      await page.click('#exportAnnotatedBtn');
      download = await waiting;
    }, async () => {});
    const size = fs.statSync(await download.path()).size;

    const result = { measured_at: new Date().toISOString(), page: opts.page, file: opts.file, download_bytes: size, waits };
    if (opts.out) fs.writeFileSync(opts.out, JSON.stringify(result, null, 2));
    console.log(JSON.stringify(result, null, 2));
  } finally {
    await browser.close();
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
