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
const { failOnPageErrors } = require('./page-health');
const { collectCoverage } = require('./coverage');
const { syntheticExport } = require('./synthetic-export');
const fs = require('fs');

const BACKEND_DIR = path.resolve(__dirname, '..', 'backend');
// Served over HTTP by the static server in playwright.config.js, the same
// way the page is served everywhere else.
const TIMELINE_HTML = 'http://127.0.0.1:8123/timeline.html';
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

// Tests share one backend process, so they cannot assume a neutral starting
// state -- detection results and uploads persist for its lifetime. Every test
// empties the stores first rather than trying to dodge what earlier tests
// left behind: a clean slate is a stronger guarantee than a distinct user id,
// which avoids collisions but leaves the old data sitting there for anything
// not keyed by user to find.
async function resetBackend() {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
}

// Still one name per test. Reset makes this unnecessary for isolation, but a
// distinct name keeps a failure message pointing at the test that produced
// the data.
let loginCounter = 0;
function uniqueSub() {
  loginCounter += 1;
  return `views-${process.pid}-${loginCounter}`;
}

// Loads the fixture through the real backend and waits for the main view.
// Returns the console errors seen during the load so tests can assert none
// occurred -- a view that renders but throws is not working.
async function loadFixture(page, { detect = false, sub = uniqueSub() } = {}) {
  const consoleErrors = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });
  page.on('pageerror', (err) => consoleErrors.push(String(err)));

  await page.goto(TIMELINE_HTML);
  await page.fill('#devLoginSub', sub);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  // Detection is opt-in now: uploading alone computes no flags at all, so
  // any test that needs them has to ask, exactly as a user would.
  if (detect) await page.check('#autoDetectCheckbox');
  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });

  return consoleErrors;
}

// Index of the first conversation the list shows as having messages. Reads
// the rendered list rather than the page's own variables, which are not
// reachable from outside once the page's script is a module.
async function firstNonEmptyConversationIndex(page) {
  return page.locator('.conv-item').evaluateAll((items) => {
    const item = items.find(
      (el) => !/^0 messages\b/.test(el.querySelector('.meta').textContent.trim())
    );
    return item ? Number(item.dataset.idx) : -1;
  });
}

// True only when this file started the server, so afterAll never stops one
// it did not start.
let startedServerHere = false;

failOnPageErrors();
collectCoverage();

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

test.beforeEach(async () => {
  await resetBackend();
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
  await expect(page.locator('.conv-item').first()).toBeVisible();

  // Pick a conversation that actually has messages rather than whichever
  // happens to be first. The fixture contains an empty conversation, and the
  // backend returns conversations in a non-deterministic order (its in-memory
  // store iterates a HashMap without sorting -- see
  // timeline-storage/src/memory/conversations.rs), so "the first one" is
  // sometimes the empty one and has no session rows to find.
  const idx = await firstNonEmptyConversationIndex(page);
  expect(idx, 'fixture had no conversation with messages').toBeGreaterThanOrEqual(0);
  await page.click(`.conv-item[data-idx="${idx}"]`);

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
  const consoleErrors = await loadFixture(page, { detect: true });

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

test('uploading without asking for detection produces no flags at all', async ({ page }) => {
  // The behavior Phase 4 exists to create: uploading a file is not consent
  // to run a pass over every message in it.
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="review"]');
  await expect(page.locator('#reviewCount')).toContainText('message');
  const checked = page.locator('#reviewTable input[type="checkbox"]:checked');
  expect(await checked.count(), 'detection ran when nobody asked for it').toBe(0);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('the detection pass reports progress while it runs', async ({ page }) => {
  const consoleErrors = [];
  page.on('console', (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text()); });
  page.on('pageerror', (err) => consoleErrors.push(String(err)));

  await page.goto(TIMELINE_HTML);
  await page.fill('#devLoginSub', uniqueSub());
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.check('#autoDetectCheckbox');

  // Capture the label's text across the whole load rather than trying to
  // catch one frame -- the fixture is small enough that the pass finishes
  // fast, and sampling for a specific instant would be inherently flaky.
  const seen = new Set();
  const poll = setInterval(async () => {
    try {
      const t = await page.locator('#loadProgressLabel').textContent();
      if (t) seen.add(t);
    } catch (e) { /* page navigating or closed; sampling is best-effort */ }
  }, 30);

  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  clearInterval(poll);

  const labels = [...seen].join(' | ');
  expect(labels, `progress labels seen: ${labels}`).toMatch(/Scanning your messages/);

  // And the pass actually did something.
  await page.click('button[data-tab="review"]');
  expect(await page.locator('#reviewTable input[type="checkbox"]:checked').count())
    .toBeGreaterThan(0);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('the upload reports byte progress before the server-side wait', async ({ page }) => {
  const consoleErrors = [];
  page.on('console', (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text()); });
  page.on('pageerror', (err) => consoleErrors.push(String(err)));

  await page.goto(TIMELINE_HTML);
  await page.fill('#devLoginSub', uniqueSub());
  await page.setInputFiles('#loadConvFile', FIXTURE);

  const seen = new Set();
  const poll = setInterval(async () => {
    try {
      const t = await page.locator('#loadProgressLabel').textContent();
      if (t) seen.add(t);
    } catch (e) { /* best-effort sampling */ }
  }, 20);

  await page.click('#loadBtn');
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  clearInterval(poll);

  const labels = [...seen].join(' | ');
  // "Processing on the server" is the honest label for the stretch after
  // the bytes are sent but before the response arrives -- the phase that
  // would otherwise look like a frozen full bar.
  expect(labels, `progress labels seen: ${labels}`).toMatch(/Processing on the server/);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('navigating writes the location to the hash, and Back returns to it', async ({ page }) => {
  // Without this the browser cannot see tab changes at all, so Back leaves
  // the page and the whole export has to be uploaded again.
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="conversations"]');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#conversations');

  await page.click('button[data-tab="analytics"]');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#analytics');

  // Choosing an analysis is its own location, so this is a third entry, not
  // a replacement of the second.
  await page.click('.analytics-item[data-analysis="friction"]');
  await expect.poll(() => page.evaluate(() => location.hash)).toBe('#analytics/friction');

  // Back steps within the page rather than leaving it: one step back is the
  // analytics tab without a chosen analysis, two is the conversations tab.
  await page.goBack();
  await expect(page.locator('#view-analytics')).toHaveClass(/active/);
  await page.goBack();
  await expect(page.locator('#view-conversations')).toHaveClass(/active/);
  await expect(page.locator('#mainContent')).toBeVisible();

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('an open conversation is addressable in the hash', async ({ page }) => {
  const consoleErrors = await loadFixture(page);

  await page.click('button[data-tab="conversations"]');
  const idx = await firstNonEmptyConversationIndex(page);
  await page.click(`.conv-item[data-idx="${idx}"]`);
  await expect.poll(() => page.evaluate(() => location.hash)).toBe(`#conversations/${idx}`);

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('reloading restores the session and says so', async ({ page }) => {
  const sub = uniqueSub();
  await loadFixture(page, { sub });

  // One list item per conversation: the search box is empty after a load.
  const before = await page.locator('.conv-item').count();
  expect(before).toBeGreaterThan(0);

  // A reload is the cheap version of the problem this solves: the export is
  // still on the server, so it should come back without picking the file
  // again.
  await page.reload();
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadScreen')).toBeHidden();
  expect(await page.locator('.conv-item').count()).toBe(before);

  // Announced, not silent -- a page that quietly opens with old data leaves
  // you unsure which file you are looking at.
  const notice = page.locator('#restoredNotice');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText(sub);

  await page.click('#restoredNoticeDismiss');
  await expect(notice).toBeHidden();
});

test('"Load a different file" stops the session coming back', async ({ page }) => {
  await loadFixture(page, { sub: uniqueSub() });
  await page.click('#loadDifferentBtn');
  await expect(page.locator('#loadScreen')).toBeVisible();

  await page.reload();
  // Back to the picker, and staying there: dismissing a session is a
  // standing decision, not one you have to repeat on every reload.
  await expect(page.locator('#loadScreen')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#restoredNotice')).toBeHidden();
});

// ---------------------------------------------------------------------------
// Paths the module split rewires, and the load-screen and save failures the
// suite above never reaches. Written against the single-file page before any
// of its script moved (docs/plans/2026-09-30-split-timeline-script.md, V4),
// so they record what the page did then and must keep passing unchanged.
// ---------------------------------------------------------------------------

// Loads any export file through the real backend, like loadFixture does for
// the checked-in fixture. `url` lets a test open the page with a query.
async function loadFile(page, file, { detect = false, sub = uniqueSub(), url = TIMELINE_HTML } = {}) {
  await page.goto(url);
  await page.fill('#devLoginSub', sub);
  await page.setInputFiles('#loadConvFile', file);
  if (detect) await page.check('#autoDetectCheckbox');
  await page.click('#loadBtn');
}

function writeExport(testInfo, name, conversations) {
  const file = testInfo.outputPath(name);
  fs.writeFileSync(file, syntheticExport(conversations));
  return file;
}

// Analyses paint a progress bar first; they are done when it is gone.
async function waitForAnalysis(page) {
  await expect(page.locator('#analyticsMain .progress-wrap')).toHaveCount(0, { timeout: 30_000 });
}

async function runAnalysis(page, name) {
  await page.click('button[data-tab="analytics"]');
  await page.click(`.analytics-item[data-analysis="${name}"]`);
  await waitForAnalysis(page);
}

const reviewBanner = (page) => page.locator('#reviewFilterBanner');

// The calendar bar with the most messages. The first bar can be a session
// holding only Claude's messages, which opens an empty review table since the
// review tab lists only yours.
async function busiestBar(page) {
  const idx = await page.locator('#calendarBody .bar').evaluateAll((bars) => {
    const count = (b) => Number((b.getAttribute('title').match(/(\d+) messages/) || [0, 0])[1]);
    return bars.reduce((best, b) => (count(b) > count(best) ? b : best)).dataset.blockIdx;
  });
  return page.locator(`#calendarBody .bar[data-block-idx="${idx}"]`);
}

async function expectReviewOpenOnSession(page) {
  await expect(page.locator('#view-review')).toHaveClass(/active/);
  await expect(reviewBanner(page)).toContainText('Time span');
}

test('ticking a flag box redraws the calendar, the conversation list and the open conversation', async ({ page }) => {
  // No detection, so nothing is flagged until the box is ticked.
  await loadFixture(page);
  const idx = await firstNonEmptyConversationIndex(page);
  await page.click('button[data-tab="conversations"]');
  await page.click(`.conv-item[data-idx="${idx}"]`);

  const calendarCritical = page.locator('#calendarBody .flag-icon.critical');
  const listCritical = page.locator(`.conv-item[data-idx="${idx}"] .flag-icon.critical`);
  await expect(calendarCritical).toHaveCount(0);
  await expect(listCritical).toHaveCount(0);
  await expect(page.locator('#convDetail')).not.toContainText('critical');

  // Opens the review tab on this conversation, so the first row is its.
  await page.click('#chatReviewLink');
  await expect(page.locator('#view-review')).toHaveClass(/active/);
  await page.locator('#reviewTable input[data-type="critical"]').first().check();

  await expect(page.locator('#saveStatus')).toHaveText('Saved.');
  await expect(page.locator('#reviewTable input[data-type="critical"]').first()).toBeChecked();
  await expect(page.locator('#reviewTable .flag-checkbox.is-override').first()).toContainText('you');
  await expect(calendarCritical).not.toHaveCount(0);
  await expect(listCritical).toHaveCount(1);
  await expect(page.locator('#convDetail')).toContainText('1 critical');
});

test('the show switches change what every view counts', async ({ page }) => {
  await loadFixture(page, { detect: true });
  const calendarFlags = page.locator('#calendarBody .flag-icon');
  const listFlags = page.locator('#convItems .flag-icon');
  const before = await calendarFlags.count();
  expect(before).toBeGreaterThan(0);
  await page.click('button[data-tab="review"]');

  // Only your own tags: nothing is yours yet, so nothing is flagged anywhere.
  await page.uncheck('#toggleShowAuto');
  await expect(calendarFlags).toHaveCount(0);
  await expect(listFlags).toHaveCount(0);
  await expect(page.locator('#reviewTable input[type="checkbox"]:checked')).toHaveCount(0);
  await expect(page.locator('#reviewTable .flag-checkbox .src').first()).toHaveText('untagged');

  await page.check('#toggleShowAuto');
  await expect(calendarFlags).toHaveCount(before);

  // Only automatic tags: read-only boxes and no Approve buttons.
  await page.uncheck('#toggleShowUser');
  await expect(page.locator('#reviewTable .approve-btn')).toHaveCount(0);
  await expect(page.locator('#reviewTable input[type="checkbox"]').first()).toBeDisabled();
  await page.check('#toggleShowUser');
  await expect(page.locator('#reviewTable .approve-btn').first()).toBeVisible();

  const replies = page.locator('#reviewTable .claude-reply-row');
  await expect(replies).toHaveCount(0);
  await page.check('#toggleShowReplies');
  expect(await replies.count()).toBeGreaterThan(0);
  await page.uncheck('#toggleShowReplies');
  await expect(replies).toHaveCount(0);
});

test('a calendar session opens the review tab on that session, and the banner widens it', async ({ page }) => {
  await loadFixture(page, { detect: true });

  // Dispatched on the bar itself so a flag icon inside it can't take the click.
  await (await busiestBar(page)).dispatchEvent('click');
  await expectReviewOpenOnSession(page);
  await expect(page.locator('#reviewTable tr.row-highlight')).not.toHaveCount(0);

  await page.click('#viewEntireConvBtn');
  await expect(reviewBanner(page)).not.toContainText('Time span');
  await expect(reviewBanner(page)).toContainText('Conversation:');

  await page.click('#clearReviewFilter');
  await expect(reviewBanner(page)).toBeHidden();
});

test('a calendar flag opens its session with the flagged messages highlighted', async ({ page }) => {
  await loadFixture(page, { detect: true });

  await page.locator('#calendarBody .bar-flags .flag-icon').first().click();
  await expectReviewOpenOnSession(page);
  await expect(page.locator('#reviewTable tr.row-highlight')).not.toHaveCount(0);
});

test('the day banner steps between days, and a session widens to its whole day', async ({ page }) => {
  await loadFixture(page);

  await (await busiestBar(page)).dispatchEvent('click');
  await expectReviewOpenOnSession(page);
  await page.click('#viewEntireDayBtn');
  await expect(reviewBanner(page)).toContainText('Day:');

  const day = reviewBanner(page).locator('strong');
  const start = await day.textContent();
  await page.click('#nextDayBtn');
  await expect(day).not.toHaveText(start);
  await page.click('#prevDayBtn');
  await expect(day).toHaveText(start);

  await page.click('#clearReviewFilter');
  await expect(reviewBanner(page)).toBeHidden();
});

test('a conversation\'s sessions, flags and review link open the review tab', async ({ page }) => {
  await loadFixture(page, { detect: true });
  await page.click('button[data-tab="conversations"]');
  await page.locator('.conv-item:has(.flag-icon)').first().click();
  await expect(page.locator('#convDetail .summary', { hasText: 'click a flag' })).toBeVisible();

  await page.locator('#convDetail .session-row').first().dispatchEvent('click');
  await expectReviewOpenOnSession(page);

  await page.click('button[data-tab="conversations"]');
  await page.locator('#convDetail .flag-icon[data-flag-type]').first().click();
  await expectReviewOpenOnSession(page);
  await expect(page.locator('#reviewTable tr.row-highlight')).not.toHaveCount(0);

  await page.click('button[data-tab="conversations"]');
  await page.click('#chatReviewLink');
  await expect(page.locator('#view-review')).toHaveClass(/active/);
  await expect(reviewBanner(page)).toContainText('Conversation:');
  await expect(reviewBanner(page)).not.toContainText('Time span');
});

test('friction ranking switches granularity and its rows open the review tab', async ({ page }) => {
  await loadFixture(page, { detect: true });
  await runAnalysis(page, 'friction');

  await page.click('#frictionBySession');
  await waitForAnalysis(page);
  await page.locator('.friction-row').first().click();
  await expectReviewOpenOnSession(page);

  await runAnalysis(page, 'friction');
  await page.click('#frictionByConv');
  await waitForAnalysis(page);
  await page.locator('.friction-row').first().click();
  await expect(page.locator('#view-review')).toHaveClass(/active/);
  await expect(reviewBanner(page)).toContainText('Conversation:');
  await expect(reviewBanner(page)).not.toContainText('Time span');
});

test('flag rate over time switches between weeks and months', async ({ page }) => {
  await loadFixture(page, { detect: true });
  await runAnalysis(page, 'trend');

  await page.click('#trendMonthly');
  await waitForAnalysis(page);
  await expect(page.locator('#trendChart svg')).toBeVisible();
  await page.click('#trendWeekly');
  await waitForAnalysis(page);
  await expect(page.locator('#trendChart svg')).toBeVisible();
});

for (const [analysis, chart] of [['length', '#lengthChart'], ['idlegap', '#idleGapChart']]) {
  test(`a point on the "${analysis}" scatter plot opens its session in the review tab`, async ({ page }) => {
    await loadFixture(page, { detect: true });
    await runAnalysis(page, analysis);

    await page.locator(`${chart} .data-point[data-idx]`).first().dispatchEvent('click');
    await expectReviewOpenOnSession(page);
  });
}

test('opening the page at an analysis address restores the session into that analysis', async ({ page }) => {
  await loadFixture(page);

  await page.evaluate(() => { location.hash = '#analytics/trend'; });
  await page.reload();
  await expect(page.locator('#restoredNotice')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#view-analytics')).toHaveClass(/active/);
  await expect(page.locator('.analytics-item[data-analysis="trend"]')).toHaveClass(/active/);
  await waitForAnalysis(page);
  await expect(page.locator('#trendChart')).toBeVisible();
  expect(await page.evaluate(() => location.hash)).toBe('#analytics/trend');
});

test('opening the page at a conversation address restores the session into that conversation', async ({ page }) => {
  await loadFixture(page);
  const idx = await firstNonEmptyConversationIndex(page);
  // The item's own text nodes hold the name; its children hold icons and counts.
  const name = await page.locator(`.conv-item[data-idx="${idx}"]`).evaluate((el) =>
    [...el.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE).map((n) => n.textContent).join('').trim());

  await page.evaluate((i) => { location.hash = `#conversations/${i}`; }, idx);
  await page.reload();
  await expect(page.locator('#restoredNotice')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#view-conversations')).toHaveClass(/active/);
  await expect(page.locator('#convDetail h3')).toHaveText(name);
  expect(await page.evaluate(() => location.hash)).toBe(`#conversations/${idx}`);
});

test('an approved flag is written into the annotated export as yours', async ({ page }) => {
  await loadFixture(page);
  await page.click('button[data-tab="review"]');
  await page.locator('.approve-btn').first().click();
  await expect(page.locator('#saveStatus')).toHaveText('Saved.');

  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.click('#exportAnnotatedBtn'),
  ]);
  const chunks = [];
  for await (const chunk of await download.createReadStream()) chunks.push(chunk);
  const parsed = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  const yours = parsed.conversations
    .flatMap((c) => c.chat_messages || [])
    .filter((m) => m._claude_timeline_user);
  expect(yours).toHaveLength(1);
});

test('a failed save says so and names the error', async ({ page }) => {
  await loadFixture(page);
  await page.route(`${API_BASE}/conversations/**/flags`, (route) =>
    route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"disk full"}' }));
  await page.click('button[data-tab="review"]');
  await page.locator('.approve-btn').first().click();
  await expect(page.locator('#saveStatus')).toHaveText(/^Could not save to the server: /);
});

test('pressing Load with no file chosen asks for one', async ({ page }) => {
  await page.goto(TIMELINE_HTML);
  await page.click('#loadBtn');
  await expect(page.locator('#loadStatus')).toHaveText('Choose a conversations.json file first.');
});

test('an export with no conversations is reported, not shown', async ({ page }, testInfo) => {
  await loadFile(page, writeExport(testInfo, 'empty.json', []));
  await expect(page.locator('#loadStatus')).toContainText('contained no conversations');
  await expect(page.locator('#loadProgressFill')).toHaveClass(/is-error/);
  await expect(page.locator('#mainContent')).toBeHidden();
});

test('a refused upload reports the server\'s own message', async ({ page }) => {
  await page.route(`${API_BASE}/uploads`, (route) =>
    route.fulfill({ status: 503, contentType: 'application/json', body: '{"error":"uploads paused"}' }));
  await loadFile(page, FIXTURE);
  await expect(page.locator('#loadStatus')).toContainText('starting the upload');
  await expect(page.locator('#loadStatus')).toContainText('uploads paused');
  await expect(page.locator('#loadProgressFill')).toHaveClass(/is-error/);
});

test('a refusal with a plain-text body reports that text', async ({ page }) => {
  await page.route(`${API_BASE}/uploads`, (route) =>
    route.fulfill({ status: 503, contentType: 'text/plain', body: 'down for maintenance' }));
  await loadFile(page, FIXTURE);
  await expect(page.locator('#loadStatus')).toContainText('starting the upload failed (503): down for maintenance');
});

test('an upload the server rejects mid-transfer reports its status', async ({ page }) => {
  await page.route(/\/_dev\/local-storage\/put\//, (route) => route.fulfill({ status: 500, body: 'no space' }));
  await loadFile(page, FIXTURE);
  await expect(page.locator('#loadStatus')).toContainText('uploading the file failed (500): no space');
});

test('an upload cut off mid-transfer suggests the backend may be down', async ({ page }) => {
  await page.route(/\/_dev\/local-storage\/put\//, (route) => route.abort());
  await loadFile(page, FIXTURE);
  await expect(page.locator('#loadStatus')).toContainText('Is the backend running');
});

test('an unreachable backend is reported with a hint', async ({ page }) => {
  await page.route(`${API_BASE}/**`, (route) => route.abort());
  await loadFile(page, FIXTURE);
  await expect(page.locator('#loadStatus')).toContainText('Is the backend running');
  await expect(page.locator('#loadProgressFill')).toHaveClass(/is-error/);
});

test('a session that cannot be fetched on reload falls back to the load screen', async ({ page }) => {
  await loadFixture(page);
  await page.route(`${API_BASE}/export`, (route) => route.abort());
  await page.reload();
  await expect(page.locator('#loadScreen')).toBeVisible();
  await expect(page.locator('#restoredNotice')).toBeHidden();
});

test('an api_base query parameter points the page at that backend', async ({ page }) => {
  // Trailing slashes are trimmed so paths can be appended directly.
  await loadFile(page, FIXTURE, { url: `${TIMELINE_HTML}?api_base=${encodeURIComponent(API_BASE + '/')}` });
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#convItems .conv-item')).not.toHaveCount(0);
});

test('Markdown in a message renders as headings, lists and code', async ({ page }, testInfo) => {
  const at = new Date('2026-03-02T15:00:00Z');
  const file = writeExport(testInfo, 'markdown.json', [{
    name: 'Markdown sample',
    messages: [{
      sender: 'human',
      at,
      text: '# A heading\n\n- first bullet\n- second bullet\n\n1. first step\n2. second step\n\nPlain with **bold** and `code`.',
    }],
  }]);
  await loadFile(page, file);
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await page.click('button[data-tab="review"]');

  const text = page.locator('#reviewTable .msg-text').first();
  await expect(text.locator('div', { hasText: 'A heading' })).toBeVisible();
  await expect(text.locator('ul li')).toHaveCount(2);
  await expect(text.locator('ol li')).toHaveCount(2);
  await expect(text.locator('strong')).toHaveText('bold');
  await expect(text.locator('code')).toHaveText('code');

  // A single session has no earlier session to measure idle time from.
  await runAnalysis(page, 'idlegap');
  await expect(page.locator('#idleGapChart')).toHaveText('Not enough data yet.');
});

test('an export holding only Claude\'s messages has nothing to chart', async ({ page }, testInfo) => {
  const file = writeExport(testInfo, 'assistant-only.json', [{
    name: 'Only replies',
    messages: [{ sender: 'assistant', at: new Date('2026-03-02T15:00:00Z'), text: 'a reply with no question' }],
  }]);
  await loadFile(page, file);
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await runAnalysis(page, 'trend');
  await expect(page.locator('#trendChart')).toHaveText('Not enough data yet.');
});

test('analyses over hundreds of messages still complete', async ({ page }, testInfo) => {
  // More than the 400 items computeWithProgress handles per animation frame,
  // spread over sessions of different lengths and flag rates.
  const conversations = [];
  const base = Date.parse('2026-01-05T14:00:00Z');
  for (let c = 0; c < 30; c++) {
    const messages = [];
    const count = 4 + (c % 7) * 5;
    for (let i = 0; i < count; i++) {
      const at = new Date(base + c * 86_400_000 + i * 60_000);
      const shouting = i % (c % 5 + 2) === 0;
      messages.push({ sender: 'human', at, text: shouting ? `this is BROKEN again, attempt ${i}` : `a calm note number ${i}` });
      messages.push({ sender: 'assistant', at: new Date(at.getTime() + 20_000), text: `reply ${i}` });
    }
    conversations.push({ name: `Synthetic ${c}`, messages });
  }
  await loadFile(page, writeExport(testInfo, 'many.json', conversations), { detect: true });
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 60_000 });

  for (const analysis of ANALYSES) {
    await runAnalysis(page, analysis);
    expect((await page.locator('#analyticsMain').innerText()).trim(), analysis).toMatch(/\d/);
  }
});
