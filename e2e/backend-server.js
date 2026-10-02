// Starts the one timeline-api server a browser-test run uses, and stops it
// afterwards. Registered as Playwright's global setup; the function it
// returns is the matching teardown.
//
// It never reuses a server. If anything already answers on the test port,
// the run stops with a message naming the port, rather than testing
// against a server of unknown age -- which is exactly what went wrong on
// 2026-10-01 (docs/plans/completed/2026-10-01-browser-tests-own-server.md).
//
// The server binary is built first and then run directly, not through
// `cargo run`, so the process this file starts is the server itself: its
// process id is the one listening on the port, which test-server.spec.js
// checks, and stopping it can't leave a child behind.

const path = require('path');
const { spawn, spawnSync } = require('child_process');
const { BACKEND_PORT, API_BASE } = require('./test-endpoints');

const BACKEND_DIR = path.resolve(__dirname, '..', 'backend');
const BINARY = path.join(BACKEND_DIR, 'target', 'debug', 'timeline-api');

// cargo and zig aren't on the default PATH on this machine; see
// backend/README.md's prerequisites.
const PATH_WITH_CARGO = [
  `${process.env.HOME}/.cargo/bin`,
  `${process.env.HOME}/.local/opt/zig`,
  process.env.PATH,
].join(':');

async function somethingAnswers(url) {
  try {
    await fetch(url);
    return true;
  } catch (e) {
    // A refused connection is the expected "nothing there" answer.
    return false;
  }
}

async function waitUntil(condition, timeoutMs, stepMs = 100) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await condition()) return true;
    await new Promise((resolve) => setTimeout(resolve, stepMs));
  }
  return false;
}

module.exports = async function startBackend() {
  if (await somethingAnswers(`${API_BASE}/conversations`)) {
    throw new Error(
      `Port ${BACKEND_PORT} is already in use, so the browser tests won't start: they only ` +
      `test a server they started themselves. Find it with \`ss -ltnp | grep :${BACKEND_PORT}\` ` +
      `and stop it.`
    );
  }

  const build = spawnSync('cargo', ['build', '-p', 'timeline-api'], {
    cwd: BACKEND_DIR,
    env: { ...process.env, PATH: PATH_WITH_CARGO },
    encoding: 'utf8',
  });
  if (build.status !== 0) {
    throw new Error(`Building timeline-api failed (exit ${build.status}):\n${build.stderr}`);
  }

  let output = '';
  let exited = null;
  const server = spawn(BINARY, [], {
    cwd: BACKEND_DIR,
    env: { ...process.env, PORT: String(BACKEND_PORT) },
    // Its own process group, so teardown can signal everything it started.
    detached: true,
  });
  server.stdout.on('data', (d) => { output += d.toString(); });
  server.stderr.on('data', (d) => { output += d.toString(); });
  server.on('exit', (code, signal) => { exited = { code, signal }; });

  const up = await waitUntil(
    async () => exited !== null || somethingAnswers(`${API_BASE}/conversations`),
    30_000
  );
  if (!up || exited !== null) {
    try { process.kill(-server.pid, 'SIGKILL'); } catch (e) { console.warn(`cleanup: ${e.message}`); }
    throw new Error(
      `timeline-api didn't start on port ${BACKEND_PORT}` +
      (exited ? ` (it exited: ${JSON.stringify(exited)})` : ' within 30 seconds') +
      `. Its output:\n${output}`
    );
  }

  // Workers inherit environment variables set here; test-server.spec.js
  // uses this to check the listener is the server this run started.
  process.env.E2E_BACKEND_PID = String(server.pid);

  return async function stopBackend() {
    try {
      process.kill(-server.pid, 'SIGTERM');
    } catch (e) {
      console.warn(`could not signal the test backend (pid ${server.pid}): ${e.message}`);
    }
    const stopped = await waitUntil(async () => !(await somethingAnswers(`${API_BASE}/conversations`)), 5_000);
    if (!stopped) {
      console.warn(`test backend still answering on port ${BACKEND_PORT} after 5s; sending SIGKILL`);
      try { process.kill(-server.pid, 'SIGKILL'); } catch (e) { console.warn(`SIGKILL failed: ${e.message}`); }
    }
  };
};
