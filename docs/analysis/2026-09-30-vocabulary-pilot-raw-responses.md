# Vocabulary pilot — raw responses

Seven replies, one per instruction variant, from the pilot described in [docs/plans/2026-09-30-vocabulary-pilot-RUN-THIS.md](../plans/2026-09-30-vocabulary-pilot-RUN-THIS.md). Each agent got exactly this prompt and nothing else:

> It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

Each reply is copied verbatim from the agent's reply message. Nothing is removed. Links inside replies are relative to the repository root, as the agents wrote them, so they do not resolve from this folder.

Before the runs, a check agent (`vocab-H`) answered: own heading "## Check names, not words" present — YES; stale heading "## Only use words that are already in the project" present — NO; and quoted its rule's first sentence correctly. It made 1 tool call even though it was told not to read any file.

---

## vocab-baseline

_Reported: 48594 tokens, 5 tool calls, 40.7 s._

Yes. The standard way is to reset the state before each test, not after it, from one shared setup step that every test file uses automatically. This repo already does the first half and is missing the second.

**What's already in place (I read this in the code):**
- `POST /_dev/reset` in /home/molinemc/workspace/conversation-timeline/backend/timeline-api/src/routes/dev_reset.rs empties every in-memory store. Only the in-memory fakes implement the `Resettable` trait (/home/molinemc/workspace/conversation-timeline/backend/timeline-storage/src/memory/resettable.rs), so the route has no way to reach real S3 or DynamoDB. /home/molinemc/workspace/conversation-timeline/backend/timeline-api/tests/lambda_router.rs checks that `_dev` routes are absent from the Lambda build.
- /home/molinemc/workspace/conversation-timeline/e2e/views.spec.js calls it in `test.beforeEach` (line 128). Resetting before rather than after is the right choice: if a test crashes before it can clean up, the next test still starts clean.

**The gap (I read this in the code):** /home/molinemc/workspace/conversation-timeline/e2e/upload-flow.spec.js has only a `beforeAll` and never resets. `resetBackend()` is defined locally in views.spec.js, so every new test file has to remember to copy it. The config runs one worker (`workers: 1` in /home/molinemc/workspace/conversation-timeline/e2e/playwright.config.js), so all files share one server in sequence. That means upload-flow's leftovers reach views.spec.js, and views.spec.js's leftovers reach upload-flow. The two tests can depend on each other depending on file order.

**The standard Playwright pattern for this:** create a custom `test` object with `test.extend` in a shared file, e.g. `e2e/test-base.js`. Give it a setup step marked `{ auto: true }` that calls `/_dev/reset`. Every spec file then imports `test` and `expect` from that file instead of from `@playwright/test`. With `auto: true`, Playwright runs the reset before every test in every file without the test asking for it. A new spec file can't forget it, and the local `beforeEach` in views.spec.js goes away. This is a built-in Playwright feature, and Playwright is Apache-2.0 and already a dependency, so nothing new gets added. The same shared file is a natural home for starting and stopping the server, which both spec files currently copy separately.

On the Rust side, the usual approach is for each test to build its own fresh in-memory stores. Then there's no shared server and nothing to reset. I didn't audit whether every file under `backend/*/tests/` does this. I only saw that `detect.rs`, `dev_routes.rs` and `export.rs` were touched when the reset route was added.

**Proposed next step:** I haven't written any code. Under the project workflow this needs a plan in /home/molinemc/workspace/conversation-timeline/docs/plans/ and user approval first. The plan would cover:
1. Add `e2e/test-base.js` with the automatic reset (and possibly the shared server start and stop).
2. Switch both spec files to import from it.
3. Remove the local `beforeEach`.
4. Confirm the suite still passes in either file order.

**Note for the caller (possibly relevant to the vocabulary pilot):** the CLAUDE.md I was given contains no vocabulary section at all. The on-disk CLAUDE.md has an uncommitted section, "## Only use words that are already in the project", that my instructions lack. I don't know which agent type I was launched as. If I was meant to be one of the `vocab-*` conditions other than baseline, the instruction variant did not reach me.

---

## vocab-A

_Reported: 47921 tokens, 6 tool calls, 42.4 s._

Yes. Playwright, the test runner already used in `e2e/`, has a standard way to do this: an **automatic fixture**. A fixture is setup code that Playwright runs around each test. If it is marked `auto: true`, every test that imports it gets the setup without asking for it.

**What the repo does now** (read in source, nothing run):
- [e2e/views.spec.js:50-53](e2e/views.spec.js#L50) defines `resetBackend()`, which calls `POST /_dev/reset`. The hook at [e2e/views.spec.js:128-130](e2e/views.spec.js#L128) runs it before each test. The comment you quoted is at [e2e/views.spec.js:44](e2e/views.spec.js#L44).
- [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets. I searched it for `reset` and `beforeEach` and found neither. It shares the same server on port 3000 as the other file, and one of its tests checks that a confirmed flag "persists through a reload" ([e2e/upload-flow.spec.js:130](e2e/upload-flow.spec.js#L130)). So today it runs on whatever state the earlier tests left behind. I haven't checked whether that has ever made a test pass or fail wrongly.
- Both spec files have their own copy of the code that starts the server (`beforeAll` with `spawn('cargo', ...)`). Only the one in `views.spec.js` first checks whether a server is already running.
- The Rust tests already do the standard thing for their level: each one builds a new router with new in-memory stores ([backend/timeline-api/tests/export.rs:45-64](backend/timeline-api/tests/export.rs#L45)). Nothing carries over between them, so they don't need a reset.

**The standard Playwright approach:**
1. **One shared fixture file**, e.g. `e2e/fixtures.js`, that wraps Playwright's `test` with `test.extend`:
   ```js
   const base = require('@playwright/test');
   exports.test = base.test.extend({
     cleanBackend: [async ({}, use) => {
       const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
       if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
       await use();
     }, { auto: true }],
   });
   exports.expect = base.expect;
   ```
   Each spec file then imports `test` from `./fixtures` instead of from `@playwright/test`. After that, every test resets first, including tests in files added later. Nobody has to remember a `beforeEach`.
2. **The `webServer` setting in [e2e/playwright.config.js](e2e/playwright.config.js)** (also built into Playwright). It starts `cargo run -p timeline-api` once for the whole run. It waits until the server's address responds, reuses a server that is already running if you ask it to, and stops the server at the end. That would replace both copied `spawn` blocks and the port-collision guards. The `PATH` additions for cargo and zig can be passed in its `env` option.

**Licence:** both features are part of `@playwright/test`, which is already a dependency and is Apache-2.0. Nothing new would be added.

**Limitation:** the reset clears every store on the server, not just one test's data. It only works because [e2e/playwright.config.js](e2e/playwright.config.js) runs tests one at a time (`workers: 1`). If tests ever run in parallel, one test's reset would wipe another's data while it is running. At that point each worker would need its own server, or state would need to be kept separate per user again.

**Next step:** following the workflow, I haven't changed any code. If you want this, I'll write a plan in `docs/plans/` covering the fixture file, the `webServer` switch and moving both spec files onto them, and wait for your approval before coding.

---

## vocab-C

_Reported: 45646 tokens, 6 tool calls, 40.7 s._

Yes, there's a standard way, and this repo already uses it in one of its two browser test files. It's also the best fit here.

**The standard way:** each test starts by wiping the server's data, instead of relying on earlier tests to clean up after themselves. The test runner does this through a hook that runs before every test (`test.beforeEach` in Playwright). The hook calls a reset route that exists only in the local test build.

**What's already in the repo** (added in commit d81b89e, "Reset the backend before each e2e test"):
- [backend/timeline-api/src/routes/dev_reset.rs](/home/molinemc/workspace/conversation-timeline/backend/timeline-api/src/routes/dev_reset.rs) adds `POST /_dev/reset`, which empties every in-memory store that implements `Resettable`. It's registered in `build_dev_router` in [backend/timeline-api/src/app.rs:55](/home/molinemc/workspace/conversation-timeline/backend/timeline-api/src/app.rs#L55). The deployed server never includes that router, and [backend/timeline-api/tests/lambda_router.rs](/home/molinemc/workspace/conversation-timeline/backend/timeline-api/tests/lambda_router.rs) checks that the route is absent there.
- [e2e/views.spec.js](/home/molinemc/workspace/conversation-timeline/e2e/views.spec.js) calls it in `test.beforeEach` through `resetBackend()`. If the reset fails, the test fails immediately rather than running on whatever data is left over. That commit measured the suite at 49.6s, against 47.6–51.8s before the change.

**Why this beats the alternatives:**
- **Wiping before each test rather than after:** a test that crashes can't leave bad data for the next one, and the leftover data stays around to inspect when a test fails.
- **Giving each test its own login name** (what the tests did before) only avoids collisions. The old data is still there, and anything not tied to a user still sees it. The comment in `dev_reset.rs` makes the same point.
- **Restarting the server for every test** would also give a clean start, but each restart means waiting for a `cargo run` start-up. The reset route is far quicker.

**One gap I found:** [e2e/upload-flow.spec.js](/home/molinemc/workspace/conversation-timeline/e2e/upload-flow.spec.js) never resets. It has no `beforeEach`, only a `beforeAll` at line 53 that starts the server. Its three tests share whatever the server holds, and the one at line 130 ("confirming a flag in the review table persists through a reload") writes data. The fix would be to add the same `resetBackend()` + `test.beforeEach` there. A small shared helper module in `e2e/` would keep the two files from carrying separate copies.

I haven't changed any code. Your workflow needs a plan in `docs/plans/` and your approval first, so I can write that plan for the upload-flow change if you want it. The only uncommitted change in the working tree is your existing edit to `CLAUDE.md`, which I didn't touch.

---

## vocab-E

_Reported: 47767 tokens, 5 tool calls, 46.8 s._

Yes. Playwright, the test runner your e2e tests use, has two built-in features for this. Between them they'd replace most of the hand-written setup code in both spec files. I haven't changed anything. Under your workflow this needs a plan first, so here's the proposal.

**What's there now**
- [e2e/views.spec.js:50](e2e/views.spec.js#L50) has a `resetBackend()` helper that calls `POST /_dev/reset`, and a `test.beforeEach` at [line 128](e2e/views.spec.js#L128) runs it before every test. The endpoint ([backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs)) empties every store registered as resettable. That part is sound.
- **[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets.** It also starts its own server on port 3000 without first checking whether one is running ([line 53](e2e/upload-flow.spec.js#L53)). Your comment in the views spec calls that exact situation a collision risk. This file's shutdown ([line 76](e2e/upload-flow.spec.js#L76)) also signals only `cargo`, not its whole process group as views does. I haven't tested whether that leaves the server running.

So the reset is a convention one file follows and the other doesn't. Nothing enforces it.

**The standard Playwright approach**

1. **An automatic fixture for the reset.** A fixture is a setup step that Playwright attaches to tests. One marked `{ auto: true }` runs before every test that imports it, whether or not the test asks for it. You put it in a shared file, e.g. `e2e/fixtures.js`:
   ```js
   const base = require('@playwright/test');
   exports.test = base.test.extend({
     cleanBackend: [async ({}, use) => {
       const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
       if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
       await use();
     }, { auto: true }],
   });
   exports.expect = base.expect;
   ```
   Each spec then imports `test` from `./fixtures` instead of from `@playwright/test`. After that, a new spec file gets the reset without having to remember it.

2. **The `webServer` setting in [e2e/playwright.config.js](e2e/playwright.config.js)** to start `cargo run -p timeline-api` once for the whole run. With `reuseExistingServer`, it uses a server that's already running instead of starting a second one. This replaces the two copies of the start, wait-for-port and shutdown code, and removes the port-3000 collision risk. Playwright starts and stops the process itself.

Together, every test starts empty no matter which file it's in or what order the files run in.

**Things to decide**
- `webServer` reuses whatever is already listening on port 3000. If a stale server from another branch is running, the tests would hit it. You could turn reuse off, or have the reset fixture also check some sign of which build it's talking to.
- Moving `upload-flow.spec.js` onto the reset could change its results if it has been passing only because of data left by earlier tests. I haven't checked.
- Want me to write this up in `docs/plans/` for your review before any code changes?

---

## vocab-G1

_Reported: 46324 tokens, 6 tool calls, 41.7 s._

Yes, there is a standard way, and most of it is already in this repository. One spec file doesn't use it yet, though.

**The standard pattern.** Gerard Meszaros's *xUnit Test Patterns* calls it a "fresh fixture". Here, "fixture" means the known state a test starts from. Each test sets up the state it needs before it runs, instead of relying on whatever earlier tests left behind. There are three usual ways to do this, from strongest to cheapest:

1. **A new instance per test.** Start a new server or database for every test. This gives the strongest isolation, but it is slow when startup is expensive. This backend takes up to 90 seconds to come up.
2. **Explicit reset before each test.** One long-lived process exposes a test-only operation that empties its stores, and a `beforeEach` hook calls it. This is what the repository does.
3. **Transaction rollback.** Each test runs inside a database transaction that is discarded afterwards. That only applies to relational storage, so it isn't relevant here.

**What the repository already does (read in source):**
- [backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs) defines a `Resettable` trait. Only the in-memory stores implement it: [conversations.rs:73](backend/timeline-storage/src/memory/conversations.rs#L73) and [object_store.rs:65](backend/timeline-storage/src/memory/object_store.rs#L65).
- [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs) exposes `POST /_dev/reset`. [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) checks that this route is absent from the build deployed to AWS Lambda.
- [e2e/views.spec.js:50-53](e2e/views.spec.js#L50-L53) defines `resetBackend()`, and [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) calls it in `test.beforeEach`.

**A gap I found.** [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) drives the same backend on port 3000 ([line 17](e2e/upload-flow.spec.js#L17)). It has a `beforeAll` hook but no `beforeEach` and no call to the reset route. I searched the file for `reset` and `beforeEach` and found neither. So its three tests still inherit whatever state earlier tests leave behind, which is exactly what the reset was added to prevent. I haven't run the suite to see whether this causes a failure today.

**What I'd recommend.** Playwright's own mechanism for this is a fixture defined with `test.extend` and marked `{ auto: true }`. Playwright then runs it before every test in every file that imports it, and no spec has to remember to add a hook. Concretely:
- Move `resetBackend()` and the shared server startup into one module, for example `e2e/backend-fixture.js`.
- Have both spec files import `test` from that module instead of from `@playwright/test`.

This closes the upload-flow gap and stops any future spec file from making the same omission. Browser-side state (cookies, `localStorage`) already starts clean, because Playwright gives each test a new browser context by default.

I haven't changed any code. Under the project workflow this needs a plan in [docs/plans/](docs/plans/) and your approval first. Should I write that plan?

---

## vocab-G2

_Reported: 43464 tokens, 4 tool calls, 28.9 s._

Yes, there is a standard way. This repo already does half of it.

**What's there now (I read this in the code):**
- The backend has a reset route, `POST /_dev/reset`, that empties the stores. The route is local-only. [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) checks that it is absent from the deployed (Lambda) build and that it really empties the stores.
- [e2e/views.spec.js:50](e2e/views.spec.js#L50) defines `resetBackend()`, and [e2e/views.spec.js:128](e2e/views.spec.js#L128) calls it in `test.beforeEach`. This came in with commit d81b89e, and the sentence you quoted is that file's code comment.
- **Gap:** [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never calls the reset. It only has a `beforeAll` at line 53. Its tests still start from whatever earlier tests left behind. So the rule "reset explicitly" is currently followed per file, and one of the two files doesn't follow it.

**The standard Playwright approach is an automatic fixture.** A fixture is a setup step that Playwright runs for a test. You declare it once with `test.extend` and mark it `{ auto: true }`. Every test that imports `test` from that shared module then gets the reset without asking. A spec can't forget it, which is exactly what happened to the upload-flow file. The shape would be:

```js
// e2e/fixtures.js
const base = require('@playwright/test');
exports.test = base.test.extend({
  cleanBackend: [async ({}, use) => {
    const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
    if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
    await use();
  }, { auto: true }],
});
exports.expect = base.expect;
```

Both spec files would then import `test` and `expect` from `./fixtures` instead of `@playwright/test`. The per-file `beforeEach` in views.spec.js would be removed. A cheap guard to go with it: a small check that fails if any `*.spec.js` imports `test` directly from `@playwright/test`. It works like the repo's other ratchet tests.

**Other options:**
- Keep a `beforeEach` in each file. This is what exists now. It works, but it relies on each author remembering, and upload-flow shows that fails.
- Use `globalSetup`, which runs once per suite. This is not enough, because you need a clean state before every test, not once per run.

**Caveat:** a full reset between tests is only safe because [e2e/playwright.config.js](e2e/playwright.config.js) pins `workers: 1`. If anyone ever runs tests in parallel against the one backend, a reset would erase data another test is using. The fixture's comment should say so.

**Not done:** per the workflow in CLAUDE.md, I haven't written a plan or any code. If the user wants this, the next step is a short plan in `docs/plans/`. It would cover the shared fixture, moving both spec files onto it, and the import guard. The user approves the plan before any code is written.

---

## vocab-H

_Reported: 46577 tokens, 5 tool calls, 34.9 s._

Yes. There is a standard way, and most of it is already in this repository. The only thing missing is that one of the two browser test files doesn't use it yet.

**The standard approach.** Each test sets up the state it needs before it runs, instead of relying on whatever earlier tests left behind. Most test frameworks provide a before-each-test step for this. Playwright's is `test.beforeEach`. There are three usual ways to get a clean state, from cheapest to most thorough:

1. **Wipe the stores before each test.** This is what the repository does now. The backend has a local-only `POST /_dev/reset` route that empties the stores it's given. [e2e/views.spec.js:50-53](e2e/views.spec.js#L50-L53) calls it, and [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) runs it before every test. When this was added (commit d81b89e), the suite took 49.6s, inside its earlier 47.6-51.8s range, so the extra call per test had no cost I could measure.
2. **Undo each test's changes afterwards.** Each test runs inside a database transaction that is rolled back at the end. This is common with SQL databases. It doesn't fit here, because this backend's stores are in memory and on local disk, not SQL, and the browser tests go through real HTTP requests.
3. **Start a new backend for each test or each file.** This gives the strongest isolation. It's expensive here, because the server is started with `cargo run`, which can take up to 90 seconds before the port answers.

Option 1 is the usual choice when the backend can offer a reset route, and it's already built. Two safety checks are in place:
- [backend/timeline-api/tests/lambda_router.rs:63](backend/timeline-api/tests/lambda_router.rs#L63) confirms that the production build (the one deployed to AWS Lambda) serves no `_dev` routes. That includes the unauthenticated reset.
- [backend/timeline-api/tests/lambda_router.rs:104](backend/timeline-api/tests/lambda_router.rs#L104) confirms that reset actually empties what it's given.

**The gap I found.** [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no reset step. From my reading of lines 53-78, it also starts its own `cargo run` without first checking whether a server is already on port 3000. It then waits for the port to answer, so it could end up testing against a different server that is already running. [e2e/playwright.config.js](e2e/playwright.config.js) runs one worker at a time because both files use that same port. The upload tests currently only upload and render, and I didn't find any assertion in them that depends on starting empty. So this is inferred risk, not an observed failure.

To apply the same approach there, I would:
- move `resetBackend()` and the "start the server unless one is already running" logic out of [e2e/views.spec.js](e2e/views.spec.js) into one shared setup module that both files import;
- run the reset from `test.beforeEach` in both files.

Playwright can also do this through a "fixture", its name for a setup step that tests request by name. That is the more idiomatic form, but it's the same mechanism.

I haven't changed any code. Under the project's workflow this change needs a plan in [docs/plans/](docs/plans/) and your approval before I write it.
