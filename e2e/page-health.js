// Fails any test during which the page threw an uncaught error, or during
// which one of the page's own script files failed to load.
//
// A module that fails to load -- a missing file, a misspelled import, a name
// that is imported but never exported -- stops the page's code from running
// with nothing more than a message in the browser console. Tests that only
// look at parts of the page the markup draws on its own could still pass, so
// every test checks for this explicitly rather than hoping an assertion
// elsewhere happens to notice.

const { test, expect } = require('./fixtures');

function failOnPageErrors() {
  let problems = [];

  test.beforeEach(async ({ page }) => {
    problems = [];
    page.on('pageerror', (err) => problems.push(`uncaught error: ${err}`));
    page.on('response', (res) => {
      if (res.url().endsWith('.js') && res.status() >= 400) {
        problems.push(`script failed to load (${res.status()}): ${res.url()}`);
      }
    });
  });

  test.afterEach(async () => {
    expect(problems, 'the page reported errors during this test').toEqual([]);
  });
}

module.exports = { failOnPageErrors };
