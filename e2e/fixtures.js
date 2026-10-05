// Playwright's `test` and `expect`, for every browser test in this folder.
// Its `page` records which of the page's own JavaScript ran, whenever the
// COVERAGE_DIR environment variable names a directory to write it to, so no
// test can be left out of the measurement
// (docs/plans/completed/2026-10-02-browser-coverage-every-test.md). Every spec file
// imports from here; coverage-setup.spec.js fails any that doesn't.
//
// Uses Chrome's built-in precise coverage through Playwright, which reports
// per-block execution counts for every script the page loaded. Each test
// writes one JSON file; coverage-report.js merges them.
//
// Chrome discards a document's coverage when the page navigates away from
// it, so what has been recorded is saved, and recording restarted, before
// page.goto, page.reload and page.close. A navigation the page starts
// itself (the sign-in redirect to Cognito and back) is NOT covered: what ran
// in the document it leaves is lost. Three ways of saving it first were
// tried on 2026-10-02 (stopping and restarting recording, or taking a
// snapshot, while holding the navigation's request with page.route, and
// holding only page-started navigations); each aborted navigations or broke
// the sign-in tests (the plan's C2). coverage-report.js names any script it
// receives without its text.

const fs = require('fs');
const path = require('path');
const base = require('@playwright/test');

const OUT = process.env.COVERAGE_DIR;
// Scripts served over http by the tests' own servers; the report matches
// them to repository files by path.
const OURS = /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?\//;

const test = base.test.extend({
  page: async ({ page }, use, testInfo) => {
    if (!OUT) {
      await use(page);
      return;
    }
    const collected = [];
    const saveSoFar = async () => {
      collected.push(...(await page.coverage.stopJSCoverage()));
      await page.coverage.startJSCoverage({ resetOnNavigation: false });
    };

    await page.coverage.startJSCoverage({ resetOnNavigation: false });
    for (const method of ['goto', 'reload', 'close']) {
      const original = page[method].bind(page);
      page[method] = async (...args) => {
        if (!page.isClosed()) await saveSoFar();
        return original(...args);
      };
    }

    await use(page);

    if (!page.isClosed()) collected.push(...(await page.coverage.stopJSCoverage()));
    fs.mkdirSync(OUT, { recursive: true });
    fs.writeFileSync(
      path.join(OUT, `${testInfo.testId}.json`),
      JSON.stringify({
        test: `${path.basename(testInfo.file)}: ${testInfo.title}`,
        entries: collected.filter((e) => OURS.test(e.url)),
      }),
    );
  },
});

module.exports = { test, expect: base.expect };
