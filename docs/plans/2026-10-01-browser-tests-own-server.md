# Browser tests start and stop their own backend, never reuse one

**Status:** done 2026-10-01. Differences from the plan below: the server binary is built and then
run directly instead of through `cargo run`, so the process the run starts is the server itself
(its process ID is the listener, and stopping it can't leave a child running); and the identity
check is its own file, [test-server.spec.js](../../e2e/test-server.spec.js). Step 4 results:
with the old server answering on port 3000 for the whole run, all 60 tests passed; with a decoy on
3123, the run stopped after 2 seconds with the "port 3123 is in use" message; after each run,
nothing was left listening on 3123.

## What went wrong

On 2026-10-01, during the V2c work, two runs of the browser tests (in [e2e/](../../e2e/)) failed:
every test that saves a flag showed "Could not save — couldn't find this message's server-side
id." The same page code and the same server program then passed twice. The page shows that
message when it has no flag handle for the message
([api-client.js](../../frontend/infra/api-client.js), `patchFlagsToBackend`).

## Theories, and how each was tested

| # | Theory | Test | Result |
|---|---|---|---|
| 1 | The tests talked to an **old server** already listening on port 3000, built before flag handles existed, so `/export` sent no handles | Build the server at commit `3b82e18` (just before handles), run it on port 3000, run two save tests | **Reproduced exactly**: both failed with the same message |
| 2 | The upload test leaves its server running afterwards (it stops `cargo run`, not the server `cargo run` started, at [upload-flow.spec.js:84](../../e2e/upload-flow.spec.js#L84)), and a later run reuses it | Run only `upload-flow.spec.js`, then check port 3000 | **Ruled out** for that file run alone: nothing left listening |
| 3 | The page code was stale (browser cache) | Not tested: each test gets a fresh browser context, and theory 1 already reproduces the exact symptom | Not needed |

**Why an old server gets used.** Both spec files treat "something answers on port 3000" as "the
server is ready":
- [views.spec.js:111-123](../../e2e/views.spec.js#L111-L123) deliberately reuses any server
  already listening.
- [upload-flow.spec.js:60-81](../../e2e/upload-flow.spec.js#L60-L81) starts `cargo run`, then waits
  for port 3000 to answer. If another server holds the port, the new one fails to start, the wait
  succeeds anyway, and every test runs against the other server.

Port 3000 is also where the dev launcher ([scripts/dev-up.sh](../../scripts/dev-up.sh)) runs your
backend. Its state file shows it started one at 2026-09-30 21:56 UTC, before flag handles
existed. Your static dev server from that session (port 8000) is still running.

**What I can't establish.** Which old server answered during the two failed runs. Your dev
backend is the likely one, but it had stopped by 03:08 UTC, and nothing in my commands between
the failed run and that check would have stopped it. If you stopped it around then, that would
confirm it.

**What this means for earlier verification.** If an old server was answering, earlier browser
runs today also ran against it, not against the code being verified. That affects the V2b
browser run and step 4 of the smaller-debug-builds plan. V2b's changes don't reach the local
server (they're in the S3 and DynamoDB code), and the build-size change doesn't change behavior,
so I don't expect different results. But those browser runs didn't verify what they claimed to.
The V2c runs that passed did use the new server: they passed tests that only a server issuing
handles can pass. Step 5 below re-runs everything.

## Fix

**A dedicated port that only the test run uses, started once per run, never reused.**

1. **One server per test run, on port 3123.** A Playwright global setup file,
   `e2e/backend-server.js`, runs before any spec file:
   - If anything already answers on port 3123, it **stops the run** with a message naming the port.
     It never reuses a server.
   - Otherwise it runs `cargo run -p timeline-api` with `PORT=3123`, in its own process group, and
     waits until the server answers.
   - The matching global teardown stops the whole process group, so the server can't outlive the
     run.

   Port 3123 follows the same idea as the static server's 8123 versus your dev server's 8000
   ([playwright.config.js](../../e2e/playwright.config.js)). Your dev backend on 3000 can never
   be mistaken for the test server.
2. **The page is pointed at port 3123** with its existing `api_base` query parameter: the test
   page address becomes `timeline.html?api_base=http://127.0.0.1:3123`. The test
   `an api_base query parameter points the page at that backend` already covers that mechanism.
3. **The spec files lose their own server code.** `views.spec.js` and `upload-flow.spec.js` stop
   starting, reusing and stopping servers, and their `API_BASE` becomes port 3123.
4. **The run proves which server it used.** Global setup records the process ID of the server it
   started. A new test checks that the process listening on 3123 is that one, so the "tests talked
   to someone else's server" failure can't recur silently.

**This changes committed test files**: the server-handling code in `views.spec.js` and
`upload-flow.spec.js`, and their port constant. No test assertion changes. Your approval of this
plan is the approval those changes need.

## Steps

1. Write `e2e/backend-server.js` (global setup and teardown) and register it in
   `playwright.config.js`.
2. Change the two spec files as above.
3. Add the "this run's server" test.
4. **Check that the fix works:**
   - Re-run experiment 1: start the old server (commit `3b82e18`) on port **3000** and run the full
     suite. It must pass, because the tests no longer look at port 3000.
   - Start any server on port **3123** and run the suite. It must stop at once with the
     "port 3123 is in use" message.
   - After a normal run, nothing may be listening on 3123.
5. Run every suite (Rust, frontend unit, browser) and commit.
6. Update [e2e/README.md](../../e2e/README.md): the port, and that the run never reuses a server.

## Self-critique log

### C1 [RESOLVED]: The fix could hide a server that fails to start
If `cargo run` fails (for example, a compile error), waiting for the port would time out with a
vague message. **Resolution:** global setup collects the server's output and prints it when the
wait fails, as `upload-flow.spec.js` already does. See [Fix, item 1 (line 50)](2026-10-01-browser-tests-own-server.md#L50).

### C2 [RESOLVED]: "Stop if the port is in use" could block a legitimate run
A crashed earlier run could leave a server on 3123. **Resolution:** the teardown stops the whole
process group, and step 4 checks nothing is left listening. If one is left anyway, the stop
message names the port, so it's easy to find and stop. See [Fix, item 1 (line 50)](2026-10-01-browser-tests-own-server.md#L50).

### C3 [OPEN]: Which old server answered on 2026-10-01 isn't known
Theory 1 reproduces the symptom, but there's no record of which process was listening during the
failed runs. **Mitigation in plan:** the fix makes the tests independent of whatever is on port
3000. **Open:** you didn't stop the dev backend (2026-10-01). I checked your session log
(`journalctl --user`) from 02:50 to 03:12 UTC and found no editor or terminal restart near 03:07;
the system log needs permissions I don't have. So what stopped it is still unknown. Trigger: the
dev backend disappearing again, at which point the launcher should record when its server exits.
