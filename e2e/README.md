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
- The Rust toolchain, per [backend/README.md](../backend/README.md) — the test starts
  `cargo run -p timeline-api` itself.

## Running

```
cd e2e
npm install
npm test
```

The test suite starts `timeline-api` in the background (polling the port, not sleeping), serves
the repo root over HTTP on port 8123 (Playwright's `webServer` setting in
[playwright.config.js](playwright.config.js)), drives `timeline.html` through the real upload flow,
and tears both servers down afterward. The page is served rather than opened as a `file://` page
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

## What's not covered

- Real AWS/LocalStack — this only ever exercises the in-memory local-dev backend. See the
  migration plan's C10 for the (separate, not-yet-built) real-adapter testing story.
- Nothing here runs as part of `cargo test --workspace` — different toolchain entirely. Treat it
  as a required manual step before calling a `timeline.html`-touching change done, the same way
  the plan already treats LocalStack/real-AWS verification for the backend.
