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

### C2 [OPEN]: navigations the page starts itself lose coverage
Chrome discards a document's coverage when the page leaves it. **Tried on 2026-10-02, all
reverted:** holding the main frame's navigation request (`page.route`) while (a) stopping and
restarting Playwright's recording, (b) taking a snapshot through Chrome's debugging protocol
(`Profiler.takePreciseCoverage`), (c) doing (b) only for navigations the page starts itself. (a)
and (b) aborted navigations (`net::ERR_ABORTED`); (c) broke the sign-in tests. Interception with no
snapshot passed (5 of 5), so the snapshot during a held navigation is what breaks it; why, not
established. **§1 item 3 is therefore not done:** recording is saved before `goto`, `reload` and
`close` only. **Effect, measured:** in [login-panel.js](../../frontend/ui/login-panel.js),
`signIn` and `signOut` (lines 76-83, 86-89) still show as never run, though the sign-in tests run
them; the file shows 45 of 57 lines (was 16 of 57 before this plan). Also unexplained: three
coverage entries for `frontend/infra/activity-recorder.js` arrived without text (in three
different tests); the report names them. **Open:** trigger is the user asking for the sign-in
redirect to be measured, or a change to the sign-in code.

## §3 Addendum (2026-10-05): unload-time recordings, and exemptions

Found after §1 was built: the report's "scripts recorded without their text" are the recorder's
send-on-close code (`leave`, `takeBatch`, `send` in
[activity-recorder.js](../../frontend/infra/activity-recorder.js)), which runs as the old page
unloads, after the fixture has saved and restarted recording, so Chrome reports it in the new
recording without the script's text. Those lines did run.

1. **Place them.** For an entry without text, the report uses the repository file's text, only when
   the same test also recorded that file *with* text and that text equals the file on disk;
   otherwise it is still named and left out.
2. **Exemptions, in the source.** Lines between `// coverage-exempt-start: <reason>` and
   `// coverage-exempt-end` are not counted; the report lists them with the reason. The user
   exempted the sign-in redirect (2026-10-05): applied to the part of `signIn` that runs before
   Cognito's page loads, in [login-panel.js](../../frontend/ui/login-panel.js) and
   [cognito-login.js](../../frontend/infra/cognito-login.js). **Not** to `signOut` or to `signIn`'s
   failure branch: those don't navigate; no test runs them, which is a test gap, not a
   measurement one.
