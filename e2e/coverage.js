// Records which of the page's own JavaScript ran during each test, when the
// COVERAGE_DIR environment variable names a directory to write it to.
// Without it this does nothing, so ordinary test runs pay no cost.
//
// Uses Chrome's built-in precise coverage through Playwright, which reports
// per-block execution counts for every script the page loaded. Each test
// writes one JSON file; coverage-report.js merges them.
//
// Chrome discards a document's coverage when the page navigates away from
// it, so a test that reloads or re-opens the page would lose everything that
// ran before. page.goto and page.reload are therefore wrapped to save what
// has been recorded so far and start recording afresh before navigating.

const fs = require('fs');
const path = require('path');
const { test } = require('@playwright/test');

const OUT = process.env.COVERAGE_DIR;
const PAGE_ORIGIN = 'http://127.0.0.1:8123/';

function collectCoverage() {
  if (!OUT) return;

  let collected = [];

  async function saveSoFar(page) {
    collected.push(...(await page.coverage.stopJSCoverage()));
    await page.coverage.startJSCoverage({ resetOnNavigation: false });
  }

  test.beforeEach(async ({ page }) => {
    collected = [];
    await page.coverage.startJSCoverage({ resetOnNavigation: false });
    for (const method of ['goto', 'reload']) {
      const original = page[method].bind(page);
      page[method] = async (...args) => {
        await saveSoFar(page);
        return original(...args);
      };
    }
  });

  test.afterEach(async ({ page }, testInfo) => {
    collected.push(...(await page.coverage.stopJSCoverage()));
    const ours = collected.filter((e) => e.url.startsWith(PAGE_ORIGIN));
    fs.mkdirSync(OUT, { recursive: true });
    fs.writeFileSync(path.join(OUT, `${testInfo.testId}.json`), JSON.stringify(ours));
  });
}

module.exports = { collectCoverage };
