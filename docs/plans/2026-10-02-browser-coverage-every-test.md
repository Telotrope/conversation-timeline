# Browser coverage: record every test, report every program file

## Context

The page's browser coverage is measured by Chrome while a test drives the page: Chrome records which
lines of each loaded script ran during that browser session, and
[e2e/coverage-report.js](../../e2e/coverage-report.js) merges the sessions. Two gaps, found on
2026-10-02 while measuring the activity-recording work:

1. **Recording is switched on per test file.** A test file records only if it calls
   `collectCoverage()` from [e2e/coverage.js](../../e2e/coverage.js). Three don't:
   [cognito-login.spec.js](../../e2e/cognito-login.spec.js),
   [hosted-page.spec.js](../../e2e/hosted-page.spec.js) and
   [test-server.spec.js](../../e2e/test-server.spec.js); no reason is recorded in the files or their
   commits. Code that runs only in those tests is reported as never run: e.g.
   [ui/login-panel.js](../../frontend/ui/login-panel.js) showed 16 of 57 lines, its sign-in
   functions "never called", though the sign-in tests run them and pass.
2. **The report lists only files some recorded session loaded.** A program file no test loads is
   absent from the report instead of shown at 0%.

Also seen: one coverage entry arrived with no source text, and the report stopped with a misleading
message ("changed between tests"); cause not established. One untested guess: Chrome discards a
page's coverage when it navigates away, and `coverage.js` saves what was recorded before `goto` and
`reload` only, not before a navigation the page starts itself (the sign-in redirect to the pretend
Cognito and back).

## §1 Changes

1. **Recording for every test, with no way to leave it out.** New
   [e2e/fixtures.js](../../e2e/fixtures.js): Playwright's `test` extended so its `page` fixture
   records coverage (the code now in `collectCoverage`, moved, with the same `goto`/`reload`
   handling). Every spec file imports `test` and `expect` from `./fixtures` instead of
   `@playwright/test`, and its `collectCoverage()` call goes. *Reuse:* the recording code is moved,
   not rewritten; `coverage.js` keeps only what the fixture uses. Changes the import line of every
   committed spec file (mechanical; what each test checks is unchanged).
2. **Enforced.** A new check, `e2e/coverage-setup.spec.js`, reads every `*.spec.js` and fails
   naming any that imports `test` from `@playwright/test` directly.
3. **Navigations the page starts itself.** Before each main-frame navigation the fixture can see
   coming (`page.on('request')` for a navigation request), it saves what was recorded. If Chrome
   still loses coverage in that case, the report's "no source" count (item 5) and the sign-in
   functions' coverage will show it; reported, not hidden.
4. **Every program file listed.** The report enumerates every `.js` file under `frontend/`
   (excluding `frontend/tests/` and `node_modules/`) and lists any that no test loaded as
   "never loaded by any test (0%)", counted in the total.
5. **Entries without source text are named, not fatal.** The report states, at the top, how many
   entries had no source and from which test and script, then reports the rest. The "changed
   between tests" error stays for its real case (two different texts for one file).
6. **Pages served from another address count too.** The hosted-page tests serve the page under
   another origin; entries are matched to repository files by path, as today the report already
   checks each script's text against the file on disk.

## §2 Tests and verification

- `coverage-setup.spec.js` itself, and the whole browser suite unchanged in what it checks.
- Rerun `COVERAGE_DIR=… npx playwright test` and the report: every `frontend/` program file listed;
  `login-panel.js`'s sign-in functions recorded as run (if not, item 3 failed and is reported).
- The report's own changes are checked by running it on that real output; no separate tests (it is
  a measuring script, like the existing one, which has none).

## Self-critique log

### C1 [RESOLVED]: a new test file could again forget to record
**Resolution:** recording is part of the `page` fixture, and §1 item 2 fails any spec that bypasses
it.

### C2 [OPEN]: navigations the page starts itself may still lose coverage
Chrome's behaviour on a navigation it doesn't announce in time is not known. **Mitigation in
plan:** §1 item 3, and the report names lost entries. **Open:** if login-panel's sign-in code still
shows as never run after this, investigate before claiming the measurement is complete.
