// Real, repeatable, driven-browser test of the read+write flow from the
// migration plan's V2a: upload a real file through the real local-dev
// timeline-api server, confirm it renders, then confirm a flag edit
// persists to the backend and survives a reload. Not a smoke test that
// merely launches the page -- every assertion is against DOM state the
// real backend produced.

const path = require('path');
const { test, expect } = require('@playwright/test');
const { spawn } = require('child_process');

const BACKEND_DIR = path.resolve(__dirname, '..', 'backend');
const TIMELINE_HTML = 'file://' + path.resolve(__dirname, '..', 'timeline.html');
const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);
const API_BASE = 'http://127.0.0.1:3000';

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

async function loadFixtureAndWaitForRender(page) {
  const consoleErrors = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });

  await page.goto(TIMELINE_HTML);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');

  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 20_000 });
  await expect(page.locator('#loadScreen')).toBeHidden();

  // The fixture's message "...pissing me off." reliably trips the drift
  // modal every load: the Rust/VADER anger detector (server-side) flags
  // it, the original AFINN+keyword-list JS detector doesn't (its phrase
  // list has "pissed," not "pissing"). Expected, documented consequence of
  // the AFINN -> VADER swap (migration plan C2), not a wiring bug -- "keep
  // the saved values" trusts the server, which is now the source of truth.
  const driftModal = page.locator('#driftModal');
  if (await driftModal.isVisible().catch(() => false)) {
    await page.click('#driftKeepSaved');
  }

  return consoleErrors;
}

test.beforeAll(async () => {
  // cargo/zig aren't on the default PATH this session installed them into
  // -- see backend/README.md's prerequisites.
  const extraPath = [
    `${process.env.HOME}/.cargo/bin`,
    `${process.env.HOME}/.local/opt/zig`,
    process.env.PATH,
  ].join(':');
  serverProcess = spawn('cargo', ['run', '-p', 'timeline-api'], {
    cwd: BACKEND_DIR,
    env: { ...process.env, PATH: extraPath },
  });
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
  if (serverProcess) serverProcess.kill('SIGTERM');
});

test('uploading a real file renders conversations from the real backend', async ({ page }) => {
  const consoleErrors = await loadFixtureAndWaitForRender(page);

  // Real conversation content from the fixture, rendered from the real
  // backend's GET /export round-trip -- not a mock, not a client-only parse.
  await expect(page.locator('#convItems')).toContainText("Google's web page caching");

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('confirming a flag in the review table persists through a reload', async ({ page }) => {
  await loadFixtureAndWaitForRender(page);

  await page.click('button[data-tab="review"]');
  const firstApprove = page.locator('.approve-btn').first();
  await expect(firstApprove).toBeVisible({ timeout: 10_000 });
  await firstApprove.click();

  // patchFlagsToBackend's success path sets this status text.
  await expect(page.locator('#saveStatus')).toHaveText('Saved.', { timeout: 10_000 });

  // Reload and re-upload the same file: if the PATCH really reached the
  // backend, the export this time embeds the confirmed override, and
  // parseUploadedConversations picks it up as an embedded override.
  await loadFixtureAndWaitForRender(page);
  // OVERRIDES is a top-level `let` in a plain (non-module) script, so it's
  // a global-scope binding, not a window property -- evaluate the bare
  // identifier, not window.OVERRIDES.
  const embeddedCount = await page.evaluate(() => Object.keys(OVERRIDES || {}).length);
  expect(embeddedCount).toBeGreaterThan(0);
});
