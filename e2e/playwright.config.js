// Drives the machine's already-installed Chrome directly, instead of
// Playwright's own bundled Chromium download -- that download needs
// `--with-deps`, which needs sudo for system libraries, and this machine's
// Chrome already has everything it needs. See the migration plan's V2a.
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: '.',
  timeout: 30_000,
  retries: 0,
  use: {
    launchOptions: {
      executablePath: '/usr/bin/google-chrome',
      args: ['--no-sandbox'],
    },
    screenshot: 'only-on-failure',
  },
});
