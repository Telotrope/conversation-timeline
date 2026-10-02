// The wait after an upload is sent, as the page shows it on AWS (plan
// docs/plans/2026-10-02-upload-processing-failures.md §3): a moving bar, and
// a line that says whether the server has started, which attempt is running
// and why the last one failed, with a clock.
//
// The local server processes an upload before answering, so its first
// status answer is already "ready". Here the status route is answered by the
// test with what the deployed API answers during a retry, then handed back
// to the real server, which says "ready".

const path = require('path');
const { test, expect } = require('@playwright/test');
const { failOnPageErrors } = require('./page-health');
const { collectCoverage } = require('./coverage');

const { API_BASE, TIMELINE_HTML } = require('./test-endpoints');
const FIXTURE = path.resolve(
  __dirname, '..', 'backend', 'timeline-core', 'tests', 'fixtures', 'sample_conversations.json'
);

failOnPageErrors();
collectCoverage();

test.beforeEach(async () => {
  const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
  if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
});

test('a retry on the server is shown, with the error, while the bar keeps moving', async ({ page }) => {
  const answers = [
    { status: 'processing' },
    { status: 'processing', attempt: 1, max_attempts: 3 },
    { status: 'processing', attempt: 1, max_attempts: 3, last_error: 'item not found' },
    { status: 'processing', attempt: 2, max_attempts: 3, last_error: 'item not found' },
  ];
  let asked = 0;
  await page.route(`${API_BASE}/uploads/*`, (route) => {
    if (route.request().method() !== 'GET' || asked >= answers.length) return route.fallback();
    const body = JSON.stringify(answers[asked]);
    asked += 1;
    return route.fulfill({ status: 200, contentType: 'application/json', body });
  });

  await page.goto(TIMELINE_HTML);
  await page.fill('#devLoginSub', `upload-wait-${process.pid}`);
  await page.setInputFiles('#loadConvFile', FIXTURE);
  await page.click('#loadBtn');

  const label = page.locator('#loadProgressLabel');
  const fill = page.locator('#loadProgressFill');
  await expect(label).toHaveText(/^Waiting for the server to start — \d+s$/);
  await expect(fill).toHaveClass(/is-working/);
  expect(await fill.evaluate((el) => getComputedStyle(el).animationName)).toBe('progressStripes');

  await expect(label).toHaveText(/^Processing on the server — \d+s$/);
  await expect(label).toHaveText(
    /^The server hit an error \(item not found\) on attempt 1 of 3 and will try again automatically in 1–2 minutes\. — \d+s$/,
  );
  await expect(label).toHaveText(
    /^The server hit an error \(item not found\) and is trying again automatically: attempt 2 of 3\. AWS waits 1–2 minutes between attempts\. — \d+s$/,
  );

  // Then the real server's "ready", and the timeline.
  await expect(page.locator('#mainContent')).toBeVisible({ timeout: 30_000 });
  expect(asked).toBe(answers.length);
});
