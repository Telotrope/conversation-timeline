// Proves the browser tests talk to the server this run started -- not
// whatever else might be listening. See backend-server.js and
// docs/plans/completed/2026-10-01-browser-tests-own-server.md.

const { execFileSync } = require('child_process');
const { test, expect } = require('@playwright/test');
const { BACKEND_PORT } = require('./test-endpoints');

test('the process listening on the test port is the server this run started', () => {
  const started = process.env.E2E_BACKEND_PID;
  expect(started, 'global setup should record the server it started').toBeTruthy();

  // `ss -p` names the process holding each listening socket.
  const listing = execFileSync('ss', ['-ltnpH', `sport = :${BACKEND_PORT}`], { encoding: 'utf8' });
  const pids = [...listing.matchAll(/pid=(\d+)/g)].map((m) => m[1]);
  expect(pids, `listeners on ${BACKEND_PORT}:\n${listing}`).toEqual([started]);
});
