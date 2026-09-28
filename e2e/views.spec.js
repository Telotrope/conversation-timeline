// Coverage for the views the upload-flow suite never opens: the calendar,
// all five analytics views, the conversation transcript, the review
// controls, and the annotated-export download.
//
// These exist because timeline.html is about to lose ~85% of its bytes
// (the embedded dictionary and sentiment lexicon, per
// docs/plans/2026-09-28-frontend-quality-of-life.md Phase 3), and a
// deletion that large needs something asserting the rest of the page still
// works. Every assertion here is against DOM state the real backend
// produced -- same real server, same real browser, same real fixture as
// upload-flow.spec.js.

const path = require('path');
const { test, expect } = require('@playwright/test');
const { spawn } = require('child_process');

const BACKEND_DIR = path.resolve(__dirname, '..', 'backend');
const TIMELINE_HTML = 'file://' + path.resolve(__dirname, '..', 'timeline.html');
const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);
const API_BASE = 'http://127.0.0.1:3000';

const ANALYSES = ['friction', 'trend', 'length', 'timeofday', 'idlegap'];

let serverProcess;
let serverOutput = '';

async function waitForPort(url, timeoutMs) {
  const start = Date.now();
  let lastError;
  while (Date.now() - start < timeoutMs) {
    try {
      await fetch(url);
      return;
    } catch (e) {
      lastError = e;
      await new Promise((resolve) => setTimeout(resolve, 300));
    }
  }
  throw new Error(`timed out waiting for ${url}: ${lastError}`);
}

// Loads the fixture through the real backend and waits for the main view.
// Returns the console errors seen during the load so tests can assert none
// occurred -- a view that renders but throws is not working.
async function loadFixture(page) {
  const consoleErrors = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });
  page.on('pageerror', (err) => consoleErrors.push(String(err)));

  await page.goto(TIMELINE_HTML);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });

  // The fixture trips the detection-drift modal on the current build; taking
  // the "keep saved" branch is what leaves the page rendering the backend's
  // own flags. The modal is being deleted in Phase 3, at which point this
  // becomes a no-op rather than a behavior change -- which is precisely why
  // these tests must pass identically before and after that deletion.
  const driftModal = page.locator('#driftModal');
  if (await driftModal.isVisible().catch(() => false)) {
    await page.click('#driftKeepSaved');
  }

  return consoleErrors;
}

// True only when this file started the server, so afterAll never stops one
// it did not start.
let startedServerHere = false;

test.beforeAll(async () => {
  // Reuse a server that is already listening rather than starting a second
  // one. Two spec files each spawning on port 3000 would collide, and a
  // second spawn would lose the bind and then silently test against the
  // first server anyway -- better to be explicit about sharing it.
  try {
    await fetch(`${API_BASE}/conversations`);
    return;
  } catch (e) {
    // Nothing listening yet, which is the normal case; start one below.
  }

  // cargo/zig aren't on the default PATH this session installed them into --
  // see backend/README.md's prerequisites.
  const extraPath = [
    `${process.env.HOME}/.cargo/bin`,
    `${process.env.HOME}/.local/opt/zig`,
    process.env.PATH,
  ].join(':');
  serverProcess = spawn('cargo', ['run', '-p', 'timeline-api'], {
    cwd: BACKEND_DIR,
    env: { ...process.env, PATH: extraPath },
    // Its own process group, so the kill in afterAll takes the actual
    // server down with cargo rather than orphaning it holding the port.
    detached: true,
  });
  startedServerHere = true;
  serverProcess.stdout.on('data', (d) => { serverOutput += d.toString(); });
  serverProcess.stderr.on('data', (d) => { serverOutput += d.toString(); });

  try {
    await waitForPort(`${API_BASE}/conversations`, 90_000);
  } catch (e) {
    console.error('timeline-api never came up. Output so far:\n', serverOutput);
    throw e;
  }
});

test.afterAll(async () => {
  if (serverProcess && startedServerHere) {
    // Negative pid signals the whole group -- see the detached spawn above.
    try {
      process.kill(-serverProcess.pid, 'SIGTERM');
    } catch (e) {
      console.warn(`could not stop the timeline-api process group: ${e.message}`);
    }
  }
});

test('the calendar renders real day rows with session bars', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  const calendar = page.locator('#calendarBody');
  await expect(calendar).toBeVisible();

  // Day rows and session bars come from BLOCKS, which is built from the
  // fixture's real timestamps -- if session-splitting broke, there'd be no
  // bars to find.
  const dayRows = calendar.locator('.day-row');
  expect(await dayRows.count()).toBeGreaterThan(0);
  const bars = calendar.locator('.bar');
  expect(await bars.count()).toBeGreaterThan(0);
  await expect(calendar.locator('.month-heading').first()).toBeVisible();

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('clicking a calendar day jumps to that day in the review tab', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.locator('#calendarBody .day-label').first().click();

  // The day label's handler switches tabs and filters the review table to
  // that date, surfacing the filter banner.
  await expect(page.locator('#view-review')).toHaveClass(/active/);
  await expect(page.locator('#reviewFilterBanner')).toBeVisible();

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('opening a conversation renders its transcript', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="conversations"]');
  const firstConv = page.locator('.conv-item').first();
  await expect(firstConv).toBeVisible();
  await firstConv.click();

  // Before the click the detail pane holds only the placeholder; after it,
  // real session rows built from the fixture. This is the only test that
  // exercises selectConversation -> renderMarkdownLite -> escapeHtml.
  const detail = page.locator('#convDetail');
  await expect(detail.locator('.placeholder')).toHaveCount(0);
  expect(await detail.locator('.session-row').count()).toBeGreaterThan(0);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

for (const analysis of ANALYSES) {
  test(`the "${analysis}" analytics view computes and renders`, async ({ page }) => {
    const consoleErrors = await loadFixture(page);

    await page.click('button[data-tab="analytics"]');
    await page.click(`.analytics-item[data-analysis="${analysis}"]`);

    // Every analysis is async and paints a progress wrapper first; the view
    // is only done when that wrapper is gone and real content replaced it.
    const main = page.locator('#analyticsMain');
    await expect(main.locator('.progress-wrap')).toHaveCount(0, { timeout: 30_000 });
    await expect(main.locator('.placeholder')).toHaveCount(0);

    const text = (await main.innerText()).trim();
    expect(text.length, `"${analysis}" rendered no content`).toBeGreaterThan(0);
    // A rendered analysis always reports at least one figure; a view that
    // silently computed nothing would still have a heading, so assert on
    // there being a digit somewhere in the output.
    expect(text, `"${analysis}" rendered no numbers`).toMatch(/\d/);

    expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
  });
}

test('review search and filter narrow the table, and pagination moves through it', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="review"]');
  const count = page.locator('#reviewCount');
  await expect(count).toContainText('message');

  const readCount = async () => {
    const text = await count.textContent();
    return parseInt(text.replace(/[^0-9]/g, ''), 10);
  };
  const unfiltered = await readCount();
  expect(unfiltered).toBeGreaterThan(0);

  // A search for something no message contains must empty the table; this
  // is what proves the filter is actually applied rather than ignored.
  await page.fill('#reviewSearch', 'zzzzzzzzzznotinanymessage');
  await expect(count).toHaveText('0 messages');

  await page.fill('#reviewSearch', '');
  await expect.poll(readCount).toBe(unfiltered);

  // "Any flag" can only ever be a subset of all messages.
  await page.selectOption('#reviewFilter', 'flagged');
  const flagged = await readCount();
  expect(flagged).toBeLessThanOrEqual(unfiltered);

  await page.selectOption('#reviewFilter', 'all');
  await expect.poll(readCount).toBe(unfiltered);

  // Pagination only exists past one page (PAGE_SIZE is 50).
  if (unfiltered > 50) {
    const firstRowBefore = await page.locator('#reviewTable tbody tr').first().innerText();
    await page.click('#nextPage');
    await expect
      .poll(async () => page.locator('#reviewTable tbody tr').first().innerText())
      .not.toBe(firstRowBefore);
  }

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('a message the backend flagged renders as flagged', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="review"]');

  // The review table renders each flag as a checkbox (see checkboxCell),
  // not as the .flag-icon glyphs the calendar and conversation list use --
  // so a checked box here is a message the backend flagged.
  //
  // These values are computed by the Rust backend during process_upload and
  // embedded in the export; nothing in the page produces them. That is the
  // property Phase 3's deletion must preserve, so if this fails after the
  // lexicons are removed, the deletion took something load-bearing with it.
  const checkedFlags = page.locator('#reviewTable input[type="checkbox"]:checked');
  expect(await checkedFlags.count(), 'no backend-computed flag rendered as checked')
    .toBeGreaterThan(0);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('the annotated export downloads a file carrying the flags', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="review"]');

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.click('#exportAnnotatedBtn'),
  ]);
  expect(download.suggestedFilename()).toBe('conversations-with-flags.json');

  const stream = await download.createReadStream();
  const chunks = [];
  for await (const chunk of stream) chunks.push(chunk);
  const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'));

  const conversations = Array.isArray(parsed) ? parsed : parsed.conversations;
  expect(Array.isArray(conversations)).toBe(true);
  expect(conversations.length).toBeGreaterThan(0);

  // The point of this file is that it round-trips the flags, so assert the
  // annotation is actually present rather than just that a file arrived.
  const annotated = conversations
    .flatMap((c) => c.chat_messages || [])
    .filter((m) => m._claude_timeline_auto);
  expect(annotated.length, 'export contained no _claude_timeline_auto fields').toBeGreaterThan(0);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});
