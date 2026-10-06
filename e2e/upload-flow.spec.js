// Real, repeatable, driven-browser test of the read+write flow from the
// migration plan's V2a: upload a real file through the real local-dev
// timeline-api server, confirm it renders, then confirm a flag edit
// persists to the backend and survives a reload. Not a smoke test that
// merely launches the page -- every assertion is against DOM state the
// real backend produced.

const path = require('path');
const { test, expect } = require('./fixtures');
const { failOnPageErrors } = require('./page-health');

const { API_BASE, TIMELINE_HTML } = require('./test-endpoints');
const { finishDescribe, signInToUpload } = require('./pages');
const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);

async function loadFixtureAndWaitForRender(page) {
  const consoleErrors = [];
  page.on('console', (msg) => {
    if (msg.type() === 'error') consoleErrors.push(msg.text());
  });

  await page.goto(TIMELINE_HTML);
  await signInToUpload(page);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');
  await finishDescribe(page);

  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 20_000 });
  await expect(page.locator('#uploadPage')).toBeHidden();

  return consoleErrors;
}

failOnPageErrors();

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

  // Sent as it is, not slimmed and compressed first (plan
  // docs/plans/2026-10-06-load-only-what-the-page-shows.md §10b), so the
  // bytes reaching the server are still over the old limit.
  // The size of each body handed to an upload request (Playwright can't read
  // a Blob body from outside the page).
  await page.addInitScript(() => {
    window.__sentSizes = [];
    const send = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function (body) {
      window.__sentSizes.push(body && body.size !== undefined ? body.size : null);
      return send.call(this, body);
    };
  });
  await page.goto(`${TIMELINE_HTML}&upload=unslimmed`);
  await signInToUpload(page);
  await page.setInputFiles('#loadConvFile', {
    name: 'large-export.json',
    mimeType: 'application/json',
    buffer,
  });
  await page.click('#loadBtn');
  await finishDescribe(page);

  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  await expect(page.locator('#loadStatus')).not.toContainText('413');
  await expect(page.locator('#convItems')).toContainText('Large upload test');
  expect(await page.evaluate(() => window.__sentSizes)).toEqual([buffer.byteLength]);
  expect(consoleErrors, `console errors:\n${consoleErrors.join('\n')}`).toEqual([]);
});

test('confirming a flag in the review table persists through a reload', async ({ page }) => {
  await loadFixtureAndWaitForRender(page);

  await page.click('button[data-tab="review"]');
  const firstApprove = page.locator('.approve-btn').first();
  await expect(firstApprove).toBeVisible({ timeout: 10_000 });
  const messageId = await firstApprove.getAttribute('data-id');
  await firstApprove.click();

  // patchFlagsToBackend's success path sets this status text.
  await expect(page.locator('#saveStatus')).toHaveText('Saved.', { timeout: 10_000 });

  // Reload and re-upload the same file: if the PATCH really reached the
  // backend, the message's row there holds your flags, and the same file
  // sent again leaves them as they are.
  await loadFixtureAndWaitForRender(page);
  await page.click('button[data-tab="review"]');
  await expect(page.locator(`#reviewTable tr[data-msg-id="${messageId}"] .review-status`)).toHaveText('Reviewed', { timeout: 10_000 });
});

test('the timeline\'s records and sessions, answered without a stated size, show what has arrived', async ({ page }) => {
  // Every text the progress label shows, kept as it changes (the reading
  // is over within a moment).
  await page.addInitScript(() => {
    window.__progressLabels = [];
    new MutationObserver(() => {
      const label = document.getElementById('loadProgressLabel');
      if (label && label.textContent) window.__progressLabels.push(label.textContent);
    }).observe(document, { subtree: true, childList: true, characterData: true });
  });
  // GET /conversations and GET /sessions, sent on to a server that streams
  // each answer in pieces with no Content-Length (Playwright's
  // route.fulfill always adds one). Plan
  // docs/plans/2026-10-06-load-only-what-the-page-shows.md §10b: opening the
  // timeline now reads these, in parts, instead of one download.
  const http = require('http');
  const streamer = http.createServer(async (req, res) => {
    if (req.method === 'OPTIONS') {
      res.writeHead(204, {
        'access-control-allow-origin': '*',
        'access-control-allow-headers': '*',
        'access-control-allow-methods': 'GET',
      });
      return res.end();
    }
    const original = await fetch(`${API_BASE}${req.url}`, { headers: { authorization: req.headers.authorization } });
    const body = Buffer.from(await original.arrayBuffer());
    res.writeHead(original.status, {
      'content-type': original.headers.get('content-type') || 'application/json',
      'access-control-allow-origin': '*',
    });
    for (let at = 0; at < body.length; at += 256) res.write(body.subarray(at, at + 256));
    res.end();
  });
  await new Promise((resolve) => streamer.listen(0, '127.0.0.1', resolve));
  const streamerBase = `http://127.0.0.1:${streamer.address().port}`;
  const sizes = [];
  const read = (url) => url.origin === API_BASE && ['/conversations', '/sessions'].includes(url.pathname);
  try {
    await page.route((url) => read(url), (route) => {
      const url = new URL(route.request().url());
      return route.continue({ url: `${streamerBase}${url.pathname}${url.search}` });
    });
    page.on('response', (res) => {
      if (res.url().startsWith(streamerBase)) sizes.push(res.headers()['content-length']);
    });

    await loadFixtureAndWaitForRender(page);

    expect(sizes.length, 'answers went through the streaming server').toBeGreaterThan(1);
    expect(sizes.filter((size) => size !== undefined), 'the browser saw no size').toEqual([]);
    const labels = await page.evaluate(() => window.__progressLabels);
    expect(labels.some((t) => /^Receiving your conversations — \d+ of \d+/.test(t)), JSON.stringify(labels)).toBe(true);
    expect(labels.some((t) => /^Receiving your sessions — \d+ of \d+/.test(t)), JSON.stringify(labels)).toBe(true);
  } finally {
    await new Promise((resolve) => streamer.close(resolve));
  }
});
