# End-to-end browser tests

Drives real `timeline.html` through a real `timeline-api` local-dev server with Playwright, per
[docs/plans/2026-09-09-rust-aws-backend-migration.md](../docs/plans/2026-09-09-rust-aws-backend-migration.md)'s
V2a section. Not a mock, not a hand-wave — a real headless browser clicking the real upload button
against a real running server, asserting on the real rendered DOM.

## Prerequisites

- **Node.js ≥ 20** (Playwright's own requirement). The Ubuntu `apt` package (`nodejs`) is only
  18.x, too old — install via [nvm](https://github.com/nvm-sh/nvm) instead:
  ```
  curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
  # in a new shell, or: source ~/.nvm/nvm.sh
  nvm install 20
  nvm use 20
  ```
- **Google Chrome** already installed at `/usr/bin/google-chrome`. `playwright.config.js` points
  at this directly (`launchOptions.executablePath`) instead of Playwright's own bundled Chromium
  download, because that download's `--with-deps` step needs `sudo` for system libraries this
  machine's Chrome already has satisfied.
- The Rust toolchain, per [backend/README.md](../backend/README.md) — the test run builds and
  starts `timeline-api` itself.

## Running

```
cd e2e
npm install
npm test
```

Before any test, [backend-server.js](backend-server.js) (Playwright's global setup) builds
`timeline-api` and starts it on **port 3123**, a port only the test run uses; your own dev backend
on port 3000 is never touched or used. **It never reuses a server**: if anything already answers on
3123, the run stops at once with a message naming the port. The page is pointed at the test
backend with its `api_base` query parameter ([test-endpoints.js](test-endpoints.js)), and
[test-server.spec.js](test-server.spec.js) checks that the process listening on 3123 is the one
the run started. This exists because on 2026-10-01 the tests silently reused an old dev backend on
port 3000 (see
[docs/plans/completed/2026-10-01-browser-tests-own-server.md](../docs/plans/completed/2026-10-01-browser-tests-own-server.md)).

The run also serves the repo root over HTTP on port 8123 (Playwright's `webServer` setting in
[playwright.config.js](playwright.config.js)), drives `timeline.html` through the real upload flow,
and stops both servers afterward. The page is served rather than opened as a `file://` page
because its scripts are JavaScript modules, which browsers refuse to load from disk. On failure,
Playwright saves a screenshot (see its own output for the path).

Every test also fails if the page throws an uncaught error or one of its script files fails to
load ([page-health.js](page-health.js)). A broken import stops the page's code from running
with only a console message, which a test looking elsewhere on the page could miss.

## What's covered

- `upload-flow.spec.js`: uploads the real test fixture
  ([backend/timeline-core/tests/fixtures/sample_conversations.json](../backend/timeline-core/tests/fixtures/sample_conversations.json))
  through the real `POST /uploads` → `PUT` → `GET /export` flow and confirms real conversation
  content renders; separately, confirms a flag confirmed via the review table's "Approve" button
  really reaches the backend (`PATCH .../flags`) and survives a reload.
- `views.spec.js`: every view the upload flow never opens (calendar, conversations, review
  controls, all five analyses, the annotated export), and the paths that cross between them:
  flag edits redrawing every view, the show switches, jumps into the review tab, opening the
  page at a conversation or analysis address, and the load-screen and save failures.
  [synthetic-export.js](synthetic-export.js) builds the exports the checked-in fixture can't
  supply (Markdown, Claude-only conversations, hundreds of messages).
- `cognito-login.spec.js`: the page pointed at a deployment (`?deploy=e2e`, its settings answered
  by the test) signs in through a pretend Cognito, [cognito-standin.js](cognito-standin.js), which
  checks the PKCE proof and hands out tokens the local backend accepts; then uploads and reloads.
  Also a refused proof, uploading before signing in, missing settings, and switching back to
  local development. Real Cognito is checked only by a deployment (migration plan §V2e, D3).
- `activity.spec.js`: the page's activity log
  ([docs/plans/2026-10-02-activity-instrumentation.md](../docs/plans/2026-10-02-activity-instrumentation.md)).
  Times the recording listener over 1,000 clicks on the review table (fails if one takes 1 ms or
  more; prints the median and average), and checks that one session's records reach the local
  backend's log in order, with one session id and no message text. The backend's standard output
  is kept in `test-results/backend-stdout.log` for this ([backend-server.js](backend-server.js)).

## Measuring coverage

With `COVERAGE_DIR` set, every test records which lines of the page's own JavaScript ran
([fixtures.js](fixtures.js): every spec file takes `test` from it, which
[coverage-setup.spec.js](coverage-setup.spec.js) checks), and [coverage-report.js](coverage-report.js)
merges the result, listing every program file under `frontend/`, including any no test loaded:

```
COVERAGE_DIR=/some/empty/dir npm test
node coverage-report.js /some/empty/dir ..
```

The last measurement, and why each remaining line is unreached, is in
[docs/analysis/2026-09-30-timeline-script-baseline-coverage.md](../docs/analysis/2026-09-30-timeline-script-baseline-coverage.md).
The page's pure data logic (`frontend/core/`) also has unit tests, run from `frontend/` with
`npm test`, alongside checks of the modules' import structure.

## What's not covered

- Real AWS and real Cognito — this only ever exercises the in-memory local-dev backend. The real
  S3 and DynamoDB adapters are tested in the Rust suite against local stand-ins (migration plan
  §V2b); real AWS is the first deployment's checks ([infra/README.md](../infra/README.md)).
- Nothing here runs as part of `cargo test --workspace` — different toolchain entirely. Treat it
  as a required manual step before calling a change to `timeline.html` or `frontend/` done, the same way
  the plan already treats LocalStack/real-AWS verification for the backend.
