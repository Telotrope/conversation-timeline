// Drives the machine's already-installed Chrome directly, instead of
// Playwright's own bundled Chromium download -- that download needs
// `--with-deps`, which needs sudo for system libraries, and this machine's
// Chrome already has everything it needs. See the migration plan's V2a.
const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
  testDir: '.',
  timeout: 30_000,
  retries: 0,
  // One worker, because every spec file drives the same real timeline-api,
  // and views.spec.js empties its stores before each test.
  workers: 1,
  // Starts this run's own timeline-api on port 3123 and stops it afterwards;
  // never reuses a server. See backend-server.js.
  globalSetup: require.resolve('./backend-server'),
  // Serves the repo root over HTTP, as the page is served in real use. The
  // page's scripts are JavaScript modules, which browsers refuse to load
  // from a file:// page. Port 8123 rather than the dev server's 8000, so a
  // running dev server is never mistaken for this one.
  webServer: {
    command: 'python3 -m http.server 8123 --bind 127.0.0.1',
    cwd: '..',
    url: 'http://127.0.0.1:8123/timeline.html',
    reuseExistingServer: false,
    stdout: 'ignore',
    stderr: 'ignore',
  },
  use: {
    launchOptions: {
      executablePath: '/usr/bin/google-chrome',
      args: ['--no-sandbox'],
    },
    screenshot: 'only-on-failure',
  },
});
