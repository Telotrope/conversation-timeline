// Real, repeatable, driven-browser test of the read+write flow from the
// migration plan's V2a: upload a real file through the real local-dev
// timeline-api server, confirm it renders, then confirm a flag edit
// persists to the backend and survives a reload. Not a smoke test that
// merely launches the page -- every assertion is against DOM state the
// real backend produced.

const path = require('path');
const { test, expect } = require('@playwright/test');
const { failOnPageErrors } = require('./page-health');
const { collectCoverage } = require('./coverage');

const { API_BASE, TIMELINE_HTML } = require('./test-endpoints');
const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);

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

  return consoleErrors;
}

failOnPageErrors();
collectCoverage();

// The server is started once per run by backend-server.js (Playwright's
// global setup), not by this file.

test('uploading a real file renders conversations from the real backend', async ({ page }) => {
  const consoleErrors = await loadFixtureAndWaitForRender(page);

  // Real conversation content from the fixture, rendered from the real
  // backend's GET /export round-trip -- not a mock, not a client-only parse.
  await expect(page.locator('#convItems')).toContainText("Google's web page caching");

  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('a file bigger than axum\'s default 2MB body limit still uploads', async ({ page }) => {
  // Regression test for a real bug: axum defaults every request body to a
  // 2MB limit. Real conversations.json exports routinely exceed that (this
  // project's own real export was 64.7MB) -- found by a user's actual load
  // failing with an unhelpfully bare 413, reproduced with a 5MB test PUT,
  // and fixed by disabling the limit on the local-dev upload route (it
  // stands in for a direct-to-S3 upload, which in production never passes
  // through this check at all -- see app.rs's build_dev_router).
  const conversations = [{
    uuid: '44444444-4444-4444-8444-444444444444',
    name: 'Large upload test',
    chat_messages: Array.from({ length: 4000 }, (_, i) => ({
      uuid: `${String(i).padStart(8, '0')}-1111-4111-8111-111111111111`,
      sender: i % 2 === 0 ? 'human' : 'assistant',
      created_at: `2024-01-01T00:${String(i % 60).padStart(2, '0')}:00Z`,
      // Varied text (not identical across messages) so dedup_chat_messages
      // doesn't collapse this into a handful of "duplicate" messages.
      content: [{ type: 'text', text: `message number ${i}: ${'x'.repeat(500)}` }],
    })),
  }];
  const buffer = Buffer.from(JSON.stringify(conversations));
  expect(buffer.byteLength).toBeGreaterThan(2 * 1024 * 1024); // actually over the old 2MB limit

  const consoleErrors = [];
  page.on('console', (msg) => { if (msg.type() === 'error') consoleErrors.push(msg.text()); });

  await page.goto(TIMELINE_HTML);
  await page.setInputFiles('#loadConvFile', {
    name: 'large-export.json',
    mimeType: 'application/json',
    buffer,
  });
  await page.click('#loadBtn');

  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadStatus')).not.toContainText('413');
  await expect(page.locator('#convItems')).toContainText('Large upload test');
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
  // The page announces confirmed flags that arrive embedded in the export,
  // and says nothing when there are none.
  await expect(page.locator('#saveStatus'))
    .toHaveText(/^Loaded [1-9]\d* of your confirmed flags? from the server\.$/);
});
