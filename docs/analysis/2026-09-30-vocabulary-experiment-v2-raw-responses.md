# Raw answers, experiment v2 (VOID as a comparison)

**Read the header before using these.** All 46 answers below were produced under the **same**
project instructions — the file as it stood when the coordinating session started, which contains
the "Only use words that are already in the project" rule. The condition labels in the filenames
(`baseline`, `A`, `C`, `E`, `G1`) record which variant was *on disk* at the time, and the variant on
disk is **not** what reached the instance. See
[the results file](2026-09-30-vocabulary-experiment-v2-results.md) for the evidence.

So: these are 46 answers to three real questions under one known instruction. They are not a
comparison of instructions, and the labels must not be read as one. They are kept because the
answers themselves are real and any later measurement can be run against them.

Each answer is verbatim, nothing removed.

---

## A-q1-r1

**Variant on disk (not what reached the instance):** A  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — Playwright has a built-in mechanism for exactly this, and the repo is
already most of the way there. Read-only answer; I ran nothing.

## What's already in place

- `POST /_dev/reset` exists and empties all four in-memory stores:
  [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs),
  the trait it dispatches through at
  [backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs),
  and the list of stores it covers at
  [backend/timeline-api/src/main.rs:92-97](backend/timeline-api/src/main.rs#L92-L97)
  (object store, conversation summaries, message flags, upload outcomes — I read
  the vector; nothing is missing from it).
- The browser tests in [e2e/views.spec.js](e2e/views.spec.js) already call it
  from a per-test hook:
  [`resetBackend` (line 50)](e2e/views.spec.js#L50) and
  [`test.beforeEach` (line 128)](e2e/views.spec.js#L128).
- The Rust integration tests don't have this problem at all. Each one builds its
  own set of in-memory stores when it builds the router — see
  [backend/timeline-api/tests/detect.rs:40-69](backend/timeline-api/tests/detect.rs#L40-L69) —
  so isolation there comes from construction, not from resetting. The
  deliberately-empty `resettable` vector at
  [detect.rs:62-66](backend/timeline-api/tests/detect.rs#L62) says so explicitly.

So the gap isn't the reset itself. It's that the reset is a hook copy-pasted into
one spec file out of two, and nothing makes a third file remember it.
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no reset hook at all
([its only hooks are `beforeAll` at line 53 and `afterAll` at line 76](e2e/upload-flow.spec.js#L53)),
and it never fills `#devLoginSub`, so every test in it runs as the default user
`alice` from [timeline.html:752](timeline.html#L752) — one shared user id, no
clean slate between tests.

## The standard mechanism: an automatic setup object (Playwright calls it a fixture)

Playwright's answer to "every test needs this setup, and no file should be able to
forget it" is `test.extend` with `{ auto: true }`, not a hook per file. One shared
module exports a wrapped `test`, and each spec requires `test`/`expect` from there
instead of from `@playwright/test`:

    // e2e/timeline-test.js  (name it after what it exports, per CLAUDE.md)
    const base = require('@playwright/test');

    async function resetBackend() {                      // moved out of views.spec.js
      const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
      if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
    }

    const test = base.test.extend({
      cleanBackend: [async ({}, use) => { await resetBackend(); await use(); },
                     { auto: true }],
    });

    module.exports = { test, expect: base.expect };

`auto: true` means it runs for every test in every file that imports this `test`,
whether or not the test mentions it. That's the property the current `beforeEach`
lacks: it can't be forgotten by a new spec file, because forgetting it means not
importing `test` at all, which doesn't compile into a runnable suite. Per
CLAUDE.md's reuse rule, this is a *move* of the existing `resetBackend`, not a
second copy — delete the hook at
[views.spec.js:128](e2e/views.spec.js#L128) in the same change.

## The matching mechanism for the server: `webServer` in the config

The other half of "can't assume a neutral starting state" is the hand-rolled
server management, duplicated in both files:
[upload-flow.spec.js:22-78](e2e/upload-flow.spec.js#L22-L78) spawns unconditionally,
while [views.spec.js:86-141](e2e/views.spec.js#L86-L141) probes the port first,
tracks `startedServerHere`, and spawns detached so it can signal the process
group. Playwright's standard replacement is one `webServer` block in
[e2e/playwright.config.js](e2e/playwright.config.js) (which currently has none —
I read all 22 lines):

    webServer: {
      command: 'cargo run -p timeline-api',
      cwd: '../backend',
      url: 'http://127.0.0.1:3000/conversations',
      reuseExistingServer: true,        // what views.spec.js hand-rolls today
      timeout: 90_000,
    },

`reuseExistingServer` is the documented equivalent of the "reuse a server that is
already listening" comment at
[views.spec.js:92-94](e2e/views.spec.js#L92-L94), and Playwright owns the
start/stop, so `startedServerHere` and the process-group kill go away. Two caveats
I'd state rather than gloss: `webServer` starts the server once per *run*, so it
resets nothing between tests — it's complementary to the automatic setup object,
not a substitute; and the `PATH` juggling for cargo/zig at
[views.spec.js:102-108](e2e/views.spec.js#L102-L108) has to move into the
`webServer` entry's `env`, or the command won't find `cargo`.

## What you do *not* need to reset

Browser-side state is already neutral per test by default: Playwright's `page`
gives each test a fresh browser context, so the `localStorage` keys
`timeline_api_base` and `timeline_dev_sub`
([timeline.html:1042-1075](timeline.html#L1042-L1075), read back at
[timeline.html:1264](timeline.html#L1264)) start empty every time. Don't add
clearing for those — the two session-restore tests at
[views.spec.js:435](e2e/views.spec.js#L435) and
[views.spec.js:460](e2e/views.spec.js#L460) depend on writing and then reading
that storage *within* one test, and anything that clears it mid-test breaks them.

## Two things to get right, or the reset makes tests worse

1. **Reset per test, never per page load.** The flag test at
   [upload-flow.spec.js:130-150](e2e/upload-flow.spec.js#L130-L150) loads the
   sample export file twice inside a single test and asserts that the second load
   sees the override the first half wrote. Putting the reset inside a load helper
   rather than in the per-test setup object would break it. The `{ auto: true }`
   shape above is the right level for this reason.
2. **Reset-between-tests and parallel workers are incompatible.** It's safe today
   only because [playwright.config.js:14](e2e/playwright.config.js#L14) pins
   `workers: 1`. With two workers against one shared server, one worker's reset
   wipes another's data mid-test. If `workers` is ever raised, the isolation story
   has to change to a per-worker server on its own port (a worker-scoped setup
   object handing each worker its own `API_BASE`) — the comment at
   [playwright.config.js:11-13](e2e/playwright.config.js#L11-L13) already names
   the port collision but not this.

## Two observations I can't resolve by reading

- A reset in [upload-flow.spec.js](e2e/upload-flow.spec.js) may turn a currently
  passing test red, and that would be information, not a regression. Its flag
  test needs `.approve-btn` rows
  ([timeline.html:2570](timeline.html#L2570)), but its load helper at
  [upload-flow.spec.js:37-51](e2e/upload-flow.spec.js#L37-L51) never checks
  `#autoDetectCheckbox`, and detection became opt-in
  ([timeline.html:1334](timeline.html#L1334)) — compare the deliberate
  `{ detect: true }` option in
  [views.spec.js:67-84](e2e/views.spec.js#L67-L84). Whether that test passes today
  on leftover flags from an earlier test or earlier run in the same server
  process, I can't tell without running it; I'm flagging it as something the reset
  would expose, not asserting it's broken.
- Minor: the startup banner at
  [backend/timeline-api/src/main.rs:108](backend/timeline-api/src/main.rs#L108)
  lists `/_dev/login` and `/_dev/local-storage/*` but not `/_dev/reset`.

Recommended order, smallest first: move `resetBackend` into the shared
`test`-exporting module and make it automatic; delete the `beforeEach`; then
replace both spawn blocks with the `webServer` entry. Per the workflow in
CLAUDE.md that's a plan in [docs/plans/](docs/plans/) before any of it gets
written.
```

---

## A-q1-r2

**Variant on disk (not what reached the instance):** A  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and it splits into two halves, because the browser half is already handled for us and the
server half is the one that needs writing.

## 1. Browser-side state: Playwright already resets it

The page keeps two things in `localStorage` — the API address and the remembered dev login name
([timeline.html:1049](timeline.html#L1049), [timeline.html:1075](timeline.html#L1075), read back at
[timeline.html:1264](timeline.html#L1264), cleared by "Load a different file" at
[timeline.html:1444](timeline.html#L1444)). Playwright hands each test its own browser context, so
each test starts with that storage empty; that is why
`reloading restores the session and says so` ([e2e/views.spec.js:434](e2e/views.spec.js#L434)) has
to do its own upload first before a reload can restore anything. I am reading this off Playwright's
isolation model plus the shape of that test, not off a measurement — I have not run anything.

Nothing to build here. If we ever want a test to *start* with a remembered name, the standard knobs
are `storageState` in the config or `context.addInitScript`, not hand-written clearing code.

## 2. Server-side state: an automatic setup step, not a copied `beforeEach`

One `timeline-api` process serves every test ([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)
pins `workers: 1` for exactly that reason), and its stores live in memory for the process's whole
life. Playwright cannot reset that, so it has to be explicit — which is what
`POST /_dev/reset` ([backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs))
already exists for, called from `resetBackend` at
[e2e/views.spec.js:50](e2e/views.spec.js#L50) via `test.beforeEach` at
[e2e/views.spec.js:128](e2e/views.spec.js#L128).

The standard way to spread that across every test file is Playwright's `test.extend` with an
always-on setup step (Playwright's own word for these is "fixtures"; the relevant part is that a
step marked `{ auto: true }` runs before every test that uses the extended `test`, whether or not
the test mentions it). Shape:

    // e2e/reset-first.js
    const base = require('@playwright/test');
    exports.expect = base.expect;
    exports.test = base.test.extend({
      cleanBackend: [async ({}, use) => {
        const res = await fetch(`${API_BASE}/_dev/reset`, { method: 'POST' });
        if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
        await use();
      }, { auto: true }],
    });

Each spec then requires `./reset-first` instead of `@playwright/test`. Why that beats the current
`test.beforeEach`: a new spec file inherits the reset by importing the same `test`, rather than by
someone remembering to write the hook. That is not hypothetical here —
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) still imports `@playwright/test` directly at
[e2e/upload-flow.spec.js:9](e2e/upload-flow.spec.js#L9), has no reset at all, and never fills
`#devLoginSub`, so all three of its tests run as `alice` (the value hard-coded into the markup at
[timeline.html:752](timeline.html#L752)). Its tests pass in declaration order because each uploads
before it asserts, but they are sharing one user id in a store nothing empties — precisely the
situation the reset route was added for.

Two related standard mechanisms worth folding in at the same time, since they are what make the
starting state knowable at all:

- **`webServer` in [e2e/playwright.config.js](e2e/playwright.config.js)** instead of each spec
  spawning `cargo run` itself. Today both files carry their own `spawn` + port-polling +
  process-group kill ([e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53),
  [e2e/views.spec.js:90](e2e/views.spec.js#L90)), and `views.spec.js` additionally hand-rolls
  "reuse a server that is already listening." `webServer` (`command`, `url`, `timeout`,
  `reuseExistingServer`) is the built-in version of all of that, run once per suite. One thing to
  carry over: the command needs the `~/.cargo/bin` and `~/.local/opt/zig` additions to `PATH` that
  both `beforeAll` blocks add, via `webServer.env`.
- **`reuseExistingServer` is a real trade-off, not a free win.** Set to `true`, the suite will
  happily run against a server someone left on port 3000 from an older binary — the staleness
  problem [scripts/dev-up.sh](scripts/dev-up.sh) records a binary hash to avoid. `false` (always
  own the process, refuse to start if the port is taken) makes the starting state unambiguous at
  the cost of killing a live local session's in-memory data, which is the thing
  [scripts/dev-up.sh](scripts/dev-up.sh) is careful about. `!process.env.CI` is the usual middle.

## Approaches I would not take, and why

- **A fresh server per test.** Perfect isolation, but a `cargo run` plus bind per test across the
  ~17 tests in [e2e/views.spec.js](e2e/views.spec.js) is seconds each. Wrong trade against wait
  time.
- **A unique user id per test only.** Already in place (`uniqueSub`,
  [e2e/views.spec.js:59](e2e/views.spec.js#L59)) and worth keeping for readable failure messages,
  but it is namespacing, not resetting — the module doc at
  [backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs)
  already makes this point: earlier tests' data stays in the store where anything not keyed by user
  can still see it.

## The Rust tests need nothing, for a structural reason

Each `#[tokio::test]` builds its own router over its own freshly-constructed stores (e.g.
`test_router` in [backend/timeline-api/tests/dev_routes.rs](backend/timeline-api/tests/dev_routes.rs)),
so there is no shared state to reset — the fresh-instance-per-test approach, which is the standard
answer when it is affordable. The one process-wide shared thing, `DEV_KEYPAIR`
([backend/timeline-api/src/dev_only.rs](backend/timeline-api/src/dev_only.rs)), is generated once
and never mutated, so it cannot leak between tests.

## One gap in the reset itself, which I would fix in the same pass

The set of stores `POST /_dev/reset` empties is a hand-maintained list at
[backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92). Add a fifth store later
and forget to push it into that vec, and the reset silently under-clears while every test still
*looks* like it starts neutral — a worse failure than no reset, because it is invisible.

What exists today: `reset_empties_the_stores_it_is_given`
([backend/timeline-api/tests/lambda_router.rs:104](backend/timeline-api/tests/lambda_router.rs#L104))
builds its own `DevState` with all four stores listed, then asserts only that the object store's one
key is gone. So the route's loop is covered; "the conversation summaries, the flags, and the upload
outcomes are also emptied" is not asserted anywhere I can find, and neither is "the list in
`main.rs` is complete." Both claims are currently believed from reading the code, not tested.

The cheap fix is one test that goes through the routes end to end against the real local wiring:
upload, run detection, confirm a flag, `POST /_dev/reset`, then assert `GET /conversations` is
empty, the flags read back empty, and `GET /export` no longer offers the old export. That pins down
all four stores at once and fails loudly if a later store is added without being registered.
```

---

## A-q1-r3

**Variant on disk (not what reached the instance):** A  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and you already built most of it — so the standard way I'd employ is finishing
the wiring rather than adding new machinery. Read-only pass, no tests run.

## What already exists

`POST /_dev/reset` empties every in-memory store, reached through the
[`Resettable`](backend/timeline-storage/src/memory/resettable.rs) trait that only the
in-memory fakes implement. The handler is
[backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs);
the list of stores it empties is assembled at
[backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92). The browser
tests in [e2e/views.spec.js:50](e2e/views.spec.js#L50) call it from a
[`test.beforeEach` at line 128](e2e/views.spec.js#L128).

The module comment on `Resettable` already makes the argument you just made — a distinct
user id per test avoids collisions but is not a clean slate, because anything not keyed
by user still sees the old data. So the design question is settled; what's left is
coverage.

## The standard mechanisms, named

Three, and they solve different halves of the problem:

1. **`test.beforeEach`** — what [e2e/views.spec.js:128](e2e/views.spec.js#L128) uses.
   It is per-file, which is exactly why it is not enough: a new spec file gets no reset
   unless its author remembers to add one. See the gap below.

2. **An automatic fixture** — Playwright's answer to "make setup unforgettable." A
   shared module exports its own `test`, extended with a fixture marked `auto: true`:

       const test = base.extend({
         cleanBackend: [async ({}, use) => { await resetBackend(); await use(); }, { auto: true }],
       });

   `auto: true` means it runs for every test in every file that imports that `test`,
   whether or not the test asks for it. Both spec files then import `test` from that
   module instead of from `@playwright/test`, and forgetting the reset stops being
   possible. This also gives the duplicated `waitForPort`/`API_BASE`/`loadFixture`
   helpers — currently copied between the two spec files — one home.

3. **`playwright.config.js`'s `webServer` option** — the standard way to own the server's
   lifecycle instead of hand-rolling it. Confirmed available: `@playwright/test` 1.63.0 is
   installed and its type definitions carry `webServer` with `url`, `command`, `cwd`,
   `timeout` and `reuseExistingServer`. Playwright starts the command, polls `url` until
   it answers, and tears it down at the end — replacing the `spawn` + `waitForPort` +
   `kill` blocks in both spec files, and removing the "which file started the server"
   question entirely.

Not the right hook: `globalSetup`. It runs once per run, not per test, so it cannot reset
between tests. It is the right place for one-time seeding, if you ever need that.

## Gaps I found while reading

**1. `e2e/upload-flow.spec.js` never resets at all.** It has no `beforeEach` — only a
`beforeAll` at [line 53](e2e/upload-flow.spec.js#L53) that spawns the server. Its three
tests share one backend *and* one user: unlike `views.spec.js`, it never fills
`#devLoginSub`, so everything lands under the same default identity. Test 1's uploaded
conversations are still in the store when tests 2 and 3 run. Today that does not turn
into a failure, because each assertion is a `toContainText` against data that test
produced itself — leftover rows sit alongside without contradicting anything. That is
luck, not isolation, and it is the concrete case the `auto: true` fixture above would
have prevented.

**2. The two files disagree about killing the server, in a way that leaks across files.**
[e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77) calls
`serverProcess.kill('SIGTERM')` on a non-detached `spawn`, which signals `cargo` — not
the `timeline-api` child holding port 3000. [e2e/views.spec.js:114](e2e/views.spec.js#L114)
spawns with `detached: true` and kills the negative pid, and its own comment says this is
why. Spec files run alphabetically, so `upload-flow` goes first; if its teardown orphans
the server, `views.spec.js`'s "reuse a server that is already listening" branch inherits
it. Its `beforeEach` reset makes that survivable — which is the argument for reset over
careful ordering, and also the argument for letting `webServer` own the lifecycle so
there is only one policy.

**3. Nothing tests that the reset list is complete.** All four in-memory stores implement
`Resettable` (`object_store`, `conversations`, `message_flags`, `uploads`) and all four are
listed at [main.rs:92](backend/timeline-api/src/main.rs#L92) today, so the list is correct
as written. But `build_local_state` lives in
[main.rs:49](backend/timeline-api/src/main.rs#L49) — the binary, not `lib.rs`, which
exports only `app`, `dev_state`, `state` and friends — so no integration test can reach
that wiring. A fifth in-memory store added later and forgotten would fail nothing.
Relatedly, the one Rust test that does exercise reset,
`reset_empties_the_stores_it_is_given` at
[backend/timeline-api/tests/lambda_router.rs:104](backend/timeline-api/tests/lambda_router.rs#L104),
builds its own four-store list but then asserts only that the object store was emptied;
the other three are in the list and never checked. (It also sits in the file about the
Lambda router, which is not where a reader would look for it.) The other suites pass
`resettable: Arc::new(vec![])` deliberately —
[dev_routes.rs:62](backend/timeline-api/tests/dev_routes.rs#L62),
[detect.rs:66](backend/timeline-api/tests/detect.rs#L66),
[export.rs:62](backend/timeline-api/tests/export.rs#L62) — with a comment saying a later
test that *does* reset should fail loudly rather than quietly clear nothing. That is a
good choice and I am not proposing to change it.

**4. The browser side already needs nothing.** `timeline.html` keeps `timeline_api_base`
and `timeline_dev_sub` in `localStorage`, but Playwright gives each test a fresh browser
context, so that storage starts empty per test without any code. Worth stating so it does
not get "fixed" redundantly.

## What I'd propose (needs your approval before I touch code)

A short plan doc in `docs/plans/`, then, in separate commits:

- Move `resetBackend`, `waitForPort`, `API_BASE` and the fixture-loading helper into one
  shared e2e module that exports a `test` extended with an `auto: true` reset fixture;
  point both spec files at it. This closes gap 1 and makes gap 1 unrepeatable.
- Replace both hand-rolled server spawns with `webServer` in
  [e2e/playwright.config.js](e2e/playwright.config.js), keeping `workers: 1` and its
  comment ([line 14](e2e/playwright.config.js#L14)) since the one-port argument still
  holds. This closes gap 2.
- For gap 3, two candidates and I'd want your call: either extend
  `reset_empties_the_stores_it_is_given` to put something in all four stores and assert
  all four are emptied (cheap, catches a broken `reset` implementation, still does not
  catch a store missing from `main.rs`'s list), or move `build_local_state` into `lib.rs`
  so a test can assert the list covers every `Resettable` implementor (catches the real
  omission, but moves code for testability — your call whether that trade is worth it).

One caveat on scope: everything above is read from source. I have not run the browser
tests, so "leakage does not currently fail `upload-flow.spec.js`" is read off the
assertions, not observed in a run.
```

---

## A-q2-r1

**Variant on disk (not what reached the instance):** A  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
# Why does the backend always detect?

Short answer: in the code as it stands, it no longer does. It used to, and you were right
about it; the behaviour was changed in the commits `5faffce` ("Delete the page's own detection
logic and its two lexicons"), `6c7d9a9` ("Move detection out of upload into a user-triggered
POST /detect") and `64996c5` ("Add upload progress, on-demand detection, and in-page
navigation"). Everything below is read from source only — I ran nothing.

You were also right that it sits badly beside the "Classify with AI" button. That part is not
fixed. Three of the problems below are wording, and two are places where the screen states
something about a flag that is not true of it.

## Why it did it, before

The migration plan asked for a free tier that is "always available": line 32-33 of
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32)
specifies "free heuristic tier (dictionary caps + sentiment/keyword criticism-anger, ~$0
marginal cost) always available" against one paid Bedrock pass per $5. What was built read
"always available" as "already computed before anyone asked", and did it inside the upload
step. Those are two different promises, which is the whole of the answer to "why".

## What happens now

- The word-list and sentiment pass has its own route, `POST /detect`
  ([backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)),
  registered at [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27).
- Uploading no longer computes anything. The doc comment at
  [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98)
  says so outright: "This does not compute flags... A freshly uploaded export therefore has no
  automatic flags until detection is requested."
- The trigger is a tick box on the load screen,
  [timeline.html:756](timeline.html#L756), with no `checked` attribute, so it starts off. It is
  read at [timeline.html:1334](timeline.html#L1334) and acted on at
  [timeline.html:1384](timeline.html#L1384) — the surrounding comment says detection "is never
  implied by the act of uploading".
- The route hands back one page of conversations at a time (default five,
  [detect.rs:43](backend/timeline-api/src/routes/detect.rs#L43)) so the page can move a
  progress bar; the loop is [timeline.html:1303](timeline.html#L1303).
- The page itself now detects nothing —
  [timeline.html:999-1001](timeline.html#L999-L1001).

## How it differs from "Classify with AI"

| | the tick box on the load screen | the Review-tab button |
|---|---|---|
| Where the work runs | Rust, on the backend ([detect.rs](backend/timeline-api/src/routes/detect.rs)) | your browser, posting straight to `api.anthropic.com` ([timeline.html:1745](timeline.html#L1745)) |
| How it decides | dictionary-checked ALL-CAPS, plus keyword and sentiment scoring ([processing.rs:84](backend/timeline-api/src/processing.rs#L84)) | Claude Sonnet reads each message with the reply above it and judges ([timeline.html:1691](timeline.html#L1691)) |
| Which of the three boxes it sets | all three: ALL-CAPS, critical, angry | two: critical and angry ([timeline.html:1848-1850](timeline.html#L1848-L1850)) |
| What it costs | nothing per message | one Sonnet call per 30 messages ([timeline.html:1690](timeline.html#L1690)) |
| Survives a reload | yes — written into stored flags and read back by `/export` ([export.rs:89-100](backend/timeline-api/src/routes/export.rs#L89-L100)) | no — the comment at [timeline.html:1597](timeline.html#L1597) says it "isn't durable across a reload/crash anymore"; only downloading the annotated file keeps it |
| Where it works at all | anywhere the backend is reachable | only while the page runs live as a Claude artifact ([timeline.html:1685-1689](timeline.html#L1685-L1689)) |
| Asks first | no, the tick is the asking | yes, a confirm box naming the message count and the number of calls ([timeline.html:1815-1821](timeline.html#L1815-L1821)) |

Both write the *same* single set of automatic values, so the second one run wins on critical
and angry. Neither touches a value you confirmed yourself — that separation is held in the
type system, see the module comment at
[backend/timeline-core/src/ports/message_flags.rs:1-18](backend/timeline-core/src/ports/message_flags.rs#L1-L18).

## What will still confuse a user

1. **The Review-tab text points at something that is not there.**
   [timeline.html:838](timeline.html#L838) reads "more accurate than the keyword/sentiment
   heuristic below". Nothing is below it in that tab any more; the word-list pass is a tick box
   on the load screen, on a different screen, before the file is even read.

2. **The two passes never mention each other.** The load screen says "scan for likely ALL-CAPS
   emphasis, criticism of Claude, and anger" ([timeline.html:757-766](timeline.html#L757-L766));
   the Review tab says "Classify with AI" ([timeline.html:835](timeline.html#L835)). Neither
   says that they fill the same three boxes, or that running the button overwrites what the tick
   box produced. The only place the difference is spelled out is a hover title at
   [timeline.html:2462](timeline.html#L2462), which nobody sees on a touch screen.

3. **After the AI pass, the ALL-CAPS box is labelled as if Claude decided it.**
   [timeline.html:1850](timeline.html#L1850) sets the source to `llm` for the whole message,
   but the lines above it only assign critical and angry. The row marker at
   [timeline.html:2479](timeline.html#L2479) then prints "AI" beside all three boxes, including
   an ALL-CAPS value that came from the dictionary pass — or from nothing at all, if the tick
   box was never ticked, in which case the box reads unticked and is labelled "AI".

4. **"auto" appears on messages nothing ever scanned.** Your own override creates the flag
   record if none exists ([memory/message_flags.rs:101](backend/timeline-storage/src/memory/message_flags.rs#L101),
   filled by `blank` at [:31](backend/timeline-storage/src/memory/message_flags.rs#L31), whose
   automatic values are all false). `/export` then writes `"source": "heuristic"` for every
   record it finds, without asking whether anything ran
   ([export.rs:99](backend/timeline-api/src/routes/export.rs#L99)). On the next load the page
   takes that source at [timeline.html:1005](timeline.html#L1005) and prints "auto" beside three
   empty boxes — which reads as "the scan looked and found nothing" when nothing looked. The
   branch that would have printed neither label is written and unreachable in this case.

5. Not detection, but it is the first sentence on the same screen as the tick box:
   [timeline.html:743](timeline.html#L743) still says "nothing is uploaded anywhere", and the
   Load button uploads the whole export.

Items 1 and 2 are wording. Items 3 and 4 are a label that says something untrue about a value,
and can be fixed without settling what the two passes should be called — which you tabled, per
[docs/plans/2026-09-28-frontend-quality-of-life.md:269-277](docs/plans/2026-09-28-frontend-quality-of-life.md#L269-L277).

## What I have not established

I read source and ran nothing, so I cannot say the current behaviour was observed, only written.
Two browser tests exist that would show it — "uploading without asking for detection produces no
flags at all" ([e2e/views.spec.js:318](e2e/views.spec.js#L318)) and "the detection pass reports
progress while it runs" ([e2e/views.spec.js:331](e2e/views.spec.js#L331)) — and I have not run
them, so I am not claiming they pass today. Items 3 and 4 are read from the assignments and the
printing code; I have not watched either label appear on screen.
```

---

## A-q2-r2

**Variant on disk (not what reached the instance):** A  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
# Why the backend always detects, and how that differs from "Classify with AI"

Read-only answer: I read the code and the project's own documents, ran nothing.

## 1. It no longer always detects — that was changed, and the change is in the code

The question was taken up as Phase 4 of
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196),
whose heading is literally "make detection an explicit step". The status table at
[docs/plans/2026-09-28-frontend-quality-of-life.md:15](docs/plans/2026-09-28-frontend-quality-of-life.md#L15)
marks it done, with one thing left over: "user-facing naming of the two tiers still tabled". That
left-over item is exactly the second half of your question, and it is still open.

What I read in the code, which agrees with that:

- [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98) —
  `process_upload` now carries the note "**This does not compute flags.**" Reading the body
  (lines 105-150 of the same file), it parses, writes one summary per conversation, records the
  outcome, and stops. Nothing writes flags there.
- The scan lives in its own place the user has to ask for:
  [backend/timeline-api/src/routes/detect.rs:72](backend/timeline-api/src/routes/detect.rs#L72),
  `POST /detect`, one page of conversations per call so the page can show real progress
  ([backend/timeline-api/src/routes/detect.rs:4](backend/timeline-api/src/routes/detect.rs#L4)
  explains that choice).
- The only thing that calls it is a box you tick on the load screen,
  [timeline.html:756](timeline.html#L756), read at
  [timeline.html:1384](timeline.html#L1384) and driven by
  [timeline.html:1303](timeline.html#L1303).

So: a freshly uploaded export now has no automatic tags at all until the box is ticked.

## 2. Why it used to

Not an oversight. [docs/plans/2026-09-09-rust-aws-backend-migration.md:32](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32)
recorded the product decision as a free scan that is "always available", set against one paid,
better-quality pass per $5. What got built read "always available" as "always already computed, at
upload, whether or not anyone asked" — which is a different claim, and is the one you objected to.
[docs/plans/2026-09-28-frontend-quality-of-life.md:198](docs/plans/2026-09-28-frontend-quality-of-life.md#L198)
says so in those terms and also says the speed argument for computing it anyway was weak and
unmeasured.

## 3. Yes, the two are genuinely different things

|  | The box on the load screen | The "Classify with AI" button |
|---|---|---|
| Where it runs | On the server, in Rust | In your browser, calling the Claude API directly |
| How it decides | A word/phrase list plus sentiment scoring, and a dictionary check so IRS and DARPA don't count as shouting ([backend/timeline-api/src/processing.rs:84](backend/timeline-api/src/processing.rs#L84)) | Sends each message, plus the preceding Claude reply, to Sonnet for a judgment ([timeline.html:1809](timeline.html#L1809), model at [timeline.html:1691](timeline.html#L1691)) |
| Costs money | No | Yes, real tokens |
| Which flags | All three: ALL-CAPS, critical, angry | Critical and angry only |
| Works where | Anywhere the server is reachable | Only while the page is running live as a Claude artifact; a downloaded copy of the file cannot reach the API at all ([timeline-project-decisions.md:388](timeline-project-decisions.md#L388)) |
| Where the result is kept | In the backend's flag store, and re-embedded into the downloadable file by [backend/timeline-api/src/routes/export.rs:94](backend/timeline-api/src/routes/export.rs#L94) | Only in the open page, until you click "Download annotated conversations.json". The recovery step that used to survive a reload was retired ([timeline.html:1590](timeline.html#L1590)) |
| Touches your own corrections | Never — separate fields by design | Never |

They aim at the same three columns in the Review tab, which is precisely why they can be mistaken
for each other.

## 4. You are right that it will confuse users, and here is where

The naming was deliberately left alone. [docs/plans/2026-09-28-frontend-quality-of-life.md:271](docs/plans/2026-09-28-frontend-quality-of-life.md#L271)
says the plan "deliberately does not invent user-facing labels, does not pair the two triggers in
the interface, and does not touch the existing auto/AI source markers." So today:

1. **Neither one is named as one of a pair.** The box says "After uploading, scan for likely ALL-CAPS
   emphasis, criticism of Claude, and anger" ([timeline.html:756-765](timeline.html#L756-L765)). The
   button says "Classify with AI" ([timeline.html:835](timeline.html#L835)). Nothing tells the reader
   these are two ways to fill the same three columns, or that the second is the better and more
   expensive one.
2. **They sit on different screens.** One is on the load screen, before anything is visible; the
   other is inside the Review tab. A user who skipped the box has no way to run the cheap scan later
   without loading the file again — the plan notes a "run it later" button was considered and left
   unbuilt ([docs/plans/2026-09-28-frontend-quality-of-life.md:235](docs/plans/2026-09-28-frontend-quality-of-life.md#L235)).
3. **The two-letter labels on each row are the only clue, and they are bare.** A row shows "auto" or
   "AI" ([timeline.html:2479](timeline.html#L2479)), explained only by a hover title
   ([timeline.html:2462](timeline.html#L2462)). "auto" does not say a word list produced it, and
   "AI" does not say it cost money. On a touch screen there is no hover at all.
4. **The load screen still says nothing is uploaded.** [timeline.html:743](timeline.html#L743) tells
   the reader "nothing is uploaded anywhere", which is no longer true of either feature; the file
   goes to the server on load, and the button sends message text to the API. This was already
   flagged as copy that must be fixed regardless
   ([docs/plans/2026-09-28-frontend-quality-of-life.md:186](docs/plans/2026-09-28-frontend-quality-of-life.md#L186)).

## 5. One thing I found while reading that is worse than a naming problem

This is read in source, not observed at runtime — I have not run it, so treat it as a thing to check
rather than a measured fact.

Ticking the load-screen box appears to throw away paid work. [backend/timeline-api/src/routes/detect.rs:120-131](backend/timeline-api/src/routes/detect.rs#L120-L131)
writes word-list flags for *every* human message in range, with no check for what was already
there; and [backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)
hard-codes `"source": "heuristic"` into every set of automatic flags it writes out. The backend's
`FlagSet` ([backend/timeline-core/src/ports/message_flags.rs:32](backend/timeline-core/src/ports/message_flags.rs#L32))
has no field for which method produced a value, so the server cannot tell the two apart even in
principle. The sequence that hurts: classify with AI, download the annotated file (it carries
`source: "llm"`), come back later, load that file with the box ticked — the word-list results
replace the paid ones and the file you download next says they came from the word list. With the box
unticked I read that the original field survives, because unknown fields are kept verbatim in
`extra` ([backend/timeline-core/src/model.rs:152](backend/timeline-core/src/model.rs#L152)) and
nothing overwrites it.

## 6. What I would suggest, though this is your call and the naming decision is yours

- Give the two things names that say what they are and put them next to each other in one place in
  the Review tab, with the load-screen box reduced to "also do the quick scan now".
- Make the per-row marker say which method and stop relying on hover.
- Give `FlagSet` a field for which method produced it, so the server stops asserting "heuristic"
  about values it did not produce, and so the scan can be made to leave better values alone.
- Fix [timeline.html:743](timeline.html#L743) either way.

I have not written any of this, and per the workflow it needs a plan you approve first.
```

---

## A-q2-r3

**Variant on disk (not what reached the instance):** A  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
# Why does the backend always detect?

Read-only answer: I read the source and the two plan documents. I ran nothing, so
everything below is "read at this line", never "measured".

## Short answer: it no longer does

The behaviour you are asking about was changed. Two commits did it:
`6c7d9a9` "Move detection out of upload into a user-triggered POST /detect" and
`64996c5` "Add upload progress, on-demand detection, and in-page navigation".

What I read in the current files:

- [backend/timeline-api/src/processing.rs:105](backend/timeline-api/src/processing.rs#L105) —
  `process_upload` reads the raw file, parses it, dedups it, writes one summary per
  conversation, records the outcome, and stops. There is no call that writes flags in
  that function. Its own doc comment says so at
  [processing.rs:98](backend/timeline-api/src/processing.rs#L98): "**This does not compute
  flags.**"
- [backend/timeline-api/src/routes/detect.rs:72](backend/timeline-api/src/routes/detect.rs#L72) —
  the three checks (ALL-CAPS with a dictionary check, criticism, anger) now run here,
  in a `POST /detect` route, over a window of conversations that the caller asks for by
  offset and count.
- [timeline.html:756](timeline.html#L756) — a checkbox on the load screen, **unticked**,
  labelled "After uploading, scan for likely ALL-CAPS emphasis, criticism of Claude, and
  anger", with the sentence "This runs only if you tick it."
- [timeline.html:1384](timeline.html#L1384) — the upload path calls the detection loop
  only inside `if(runDetection)`, which is that checkbox's state read at
  [timeline.html:1334](timeline.html#L1334).
- [timeline.html:1303](timeline.html#L1303) — the loop asks for five conversations at a
  time so the bar can move, and the label reads "Scanning your messages — N of M
  conversations".
- [timeline.html:999-1001](timeline.html#L999-L1001) — the page's own comment: it does no
  detection itself, and a message with no stored automatic flags "simply has none --
  which is the normal state until the user asks for a detection pass."

So: a fresh upload now arrives with no automatic flags at all, and the timeline,
calendar and conversation list render without them.

## Why it used to detect on every upload

Not an oversight. It came from a decision recorded at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:32](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32):
a free tier of the keyword/dictionary/sentiment checks, "~$0 marginal cost", "always
available", as against one paid pass of better quality per $5. What got built read
"always available" as "already computed, at upload, whether or not anyone asked". Those
are two different promises, and your complaint is about the second one. The plan's own status table marks that change **Done**, with "user-facing naming of the two
tiers still tabled" ([docs/plans/2026-09-28-frontend-quality-of-life.md:15](docs/plans/2026-09-28-frontend-quality-of-life.md#L15))
— that is the plan's claim about itself, and separate from my reading of the code above.
The reasoning and the decision to change it are written up at
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196).

## Is it different from "Classify with AI"? Yes, in six ways

Both write into the same automatic slot and both leave anything you confirmed yourself
alone. Everything else differs.

| | The detection pass | "Classify with AI" |
|---|---|---|
| Where the work happens | Rust, in the backend ([routes/detect.rs](backend/timeline-api/src/routes/detect.rs)) | In your browser, calling `api.anthropic.com` ([timeline.html:1809](timeline.html#L1809)) |
| How it decides | A dictionary check for ALL-CAPS, plus keyword and sentiment scoring for criticism and anger ([processing.rs:84](backend/timeline-api/src/processing.rs#L84)) | Claude Sonnet is asked to judge each message, with the preceding Claude reply for context ([timeline.html:1691](timeline.html#L1691)) |
| What it writes | All three: ALL-CAPS, critical, angry | Two only: critical and angry. ALL-CAPS is left as it was ([timeline.html:1848](timeline.html#L1848)) |
| Cost | No charge | Real tokens — around 30 messages per call, and the confirmation box states the number of calls before you agree |
| Where to start it | The checkbox on the load screen, before the upload | A button in the "Review & flags" tab ([timeline.html:844](timeline.html#L844)) |
| Where it works | Anywhere the backend is reachable | Only while the page is open live as a Claude artifact; a downloaded copy of the file cannot reach that address ([timeline-project-decisions.md:278](timeline-project-decisions.md#L278) and :388) |

In the Review table the two show up as different one-word labels next to a flag: "auto"
for the backend pass, "AI" for the Claude pass, with a hover title that spells each one
out ([timeline.html:2462](timeline.html#L2462), [timeline.html:2479](timeline.html#L2479)).

## You are right that this will confuse users, and here is what I would point at

1. **Nothing in the interface tells anyone that these are two grades of the same
   judgment.** The two triggers sit in different places (load screen, Review tab), they
   are worded independently, and the only place the relationship is stated is the
   sentence "meant to be more accurate than the keyword/sentiment heuristic below"
   inside the Classify box ([timeline.html:838](timeline.html#L838)) — which names the
   other pass only as "the heuristic", a word that appears nowhere else on screen. The
   plan explicitly left the naming of the two grades undecided and says you tabled it
   ([docs/plans/2026-09-28-frontend-quality-of-life.md:271](docs/plans/2026-09-28-frontend-quality-of-life.md#L271)).
   That is still open, and it is the substance of your complaint.
2. **The load screen still says "nothing is uploaded anywhere"**
   ([timeline.html:743](timeline.html#L743)). The file is uploaded, to the backend, on
   every load. The Phase 3 notes already flag this line as false and say it must be
   fixed regardless. It is still there.
3. **The same sentence promises something the backend does not honour.** It offers "a
   file this page previously saved for you (with your flags already in it)". But the page
   renders from what the backend hands back, not from the file you chose
   ([timeline.html:1390-1416](timeline.html#L1390-L1416)), and `process_upload` stores no
   flags from an uploaded file. `GET /export` writes `_claude_timeline_auto` only when
   the backend's own store has a record
   ([routes/export.rs:89-104](backend/timeline-api/src/routes/export.rs#L89-L104)). Read
   that way, re-loading a file you downloaded from this page drops the flags embedded in
   it. I did not run this; it is what the two call paths say.
4. **A Claude-judged flag does not survive a reload.** `classifyWithAI` updates the page's
   own arrays and checkpoints to the artifact storage, and the comment at
   [timeline.html:1592-1600](timeline.html#L1592-L1600) states the load-time recovery step
   was retired, so the result is not durable. Nothing sends it to the backend. And the
   export route writes `"source": "heuristic"` as a constant
   ([routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)), so even if a
   record existed the "AI" label could not come back through it. A user who pays tokens,
   sees "AI" next to their flags, then reloads, will see "auto" or nothing.

## What I did not check

I read source and comments only. I did not start the backend, did not upload anything,
and did not run the tests, so none of the above is a statement about observed runtime
behaviour — in particular items 3 and 4, which are conclusions drawn from reading two
call paths and would each be settled by one real load.
```

---

## A-q3-r1

**Variant on disk (not what reached the instance):** A  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work left off, and what is verified

Read-only review: I read the plans, the analysis document, the crate READMEs, and the source
files named below. I ran no tests and wrote no code, so every claim about a test *result* here is
a claim about what a document says, not something I measured.

## Two tracks, and the live one is not the product

**The product code stopped at commit `d81b89e` ("Reset the backend before each e2e test").** The
four commits after it — `af47dae`, `54c9e19`, `57c8d3b`, `966655d` — are all about a side
experiment on the wording of [CLAUDE.md](CLAUDE.md) itself, not about the timeline tool.

**That experiment is mid-flight right now**, and it is what the working tree is holding:

- [CLAUDE.md](CLAUDE.md) has an uncommitted new `## Vocabulary` section reading "Write at the
  level of language you would expect from someone familiar with object-oriented software
  engineering texts and practice", plus a one-line wording change in the workflow section
  ("finish work on an *already-approved* plan").
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  has uncommitted edits replacing two of its three questions and adding the constraint that every
  question must be answerable by reading alone.
- None of the three files that plan says it will produce exist yet. [docs/analysis/](docs/analysis/)
  holds only the first attempt's write-up,
  [2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md).
  So the second attempt is planned and its instruction text is installed, but it has not been run.

Worth knowing about the first attempt, from its own write-up: 20 runs were launched and **all 20
were lost** when the coordinating session ended; a second pass completed 6 runs against a target
of 30. Its two findings are that the strict word-lookup rule made the writing worse while scoring
best on its own measure, and that the rule as written into [CLAUDE.md](CLAUDE.md) forbids ordinary
words like *but* and *not* on a literal reading. Both timing and word-count conclusions there rest
on two runs per condition.

## The product code: where it stands

**Two plans govern it.**
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
runs from a pure-logic crate through deployment, payment and hardening.
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
is a six-phase pass over the page, meant to finish before returning to the server work.

**All six phases of the quality-of-life plan are marked Done** in its own status table: the dev
launcher and editor tasks, browser-test coverage for the calendar and analytics views, the
deletion of the page's own detection code and its two word lists, detection behind an explicit
request with visible progress, an upload progress bar, and back/forward navigation with
restore-on-load.

I confirmed the large deletion by reading the file rather than trusting the table:
[timeline.html](timeline.html) is now 3,110 lines / 119 KB, and `DICTIONARY_WORDS_RAW`, the
sentiment word list, the drift modal and `showDriftModal` are all gone. The false reassurance
that the export "is read locally in this browser tab and is never uploaded anywhere" is also gone.

Detection now runs only when asked: [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)
serves a paged pass so the page can drive a real progress bar, and
[backend/timeline-api/src/processing.rs](backend/timeline-api/src/processing.rs) no longer writes
flags at upload time.

**On the migration plan, the first two versions have landed and the rest has not started.** The
four crates exist ([backend/timeline-core](backend/timeline-core), [backend/timeline-storage](backend/timeline-storage),
[backend/timeline-auth](backend/timeline-auth), [backend/timeline-api](backend/timeline-api)), the
browser talks to the real server through the local-only routes, and
[backend/timeline-api/src/main.rs](backend/timeline-api/src/main.rs) wires **only the in-memory
adapters** — I read it; the real S3 and DynamoDB adapters are not constructed anywhere in it.
Nothing of the classification work, the payment work, or the hardening work has begun.

## Verified by running the actual code

Taken from [backend/README.md](backend/README.md)'s own honest split, which I read rather than
re-measured:

- The local server driven by hand with `curl`, and the same paths captured as committed tests in
  [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs) against the real router:
  unauthenticated and garbage-token requests rejected, upload creation returning a real signed-URL
  shape, a flag written and read back.
- Token verification in [backend/timeline-auth](backend/timeline-auth), including every rejection
  path, against a real self-signed keypair and real signing.
- The in-memory adapters, at 100% line coverage, through their public traits — including that
  automatic flags and the user's own flags cannot cross-contaminate.
- A real ARM64 Lambda binary cross-compiled, then exercised through a local emulation of the
  Lambda runtime with genuine API-Gateway-shaped events. **Manual session, not a committed test.**
- The browser tests drive a real browser against a real running server: 3 tests in
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and 13 in [e2e/views.spec.js](e2e/views.spec.js).
- [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  exactly what the Lambda branch builds and asserts every local-only path returns 404 while the
  real routes are present — the one guard against shipping the token-minting and
  erase-everything routes to production.

## Not verified

- **No `send()` call in the real AWS adapters has ever reached AWS or any emulator.**
  [backend/timeline-storage/README.md](backend/timeline-storage/README.md) records
  [s3.rs](backend/timeline-storage/src/s3.rs) at 0% coverage,
  [dynamo/conversations_table.rs](backend/timeline-storage/src/dynamo/conversations_table.rs) at 0%,
  and [dynamo/message_flags_table.rs](backend/timeline-storage/src/dynamo/message_flags_table.rs)
  at 63.79% covering only the pure expression-building logic, through temporary private-function
  tests. This is the migration plan's C10, tagged `[OPEN]` with no mitigation: a container-free
  local DynamoDB is available as a plain Java archive, and there is still no licence-clean local
  substitute for S3.
- **[infra/template.yaml](infra/template.yaml) has never been validated or deployed.** It is a
  193-line template describing the bucket, three tables, the user pool, the function and the HTTP
  API — all on paper.
- **Nothing has run against a real user pool.** Only the throwaway keypair stands in for one.
- **The Lambda binary has never been deployed to the real service.**
- **There is no continuous integration of any kind** — no `.github` directory exists. Every suite
  runs only when someone remembers to run it.
- **`classifyWithAI` is still in [timeline.html](timeline.html)** (lines 1809 and 2633), untested,
  artifact-only, and slated for replacement by the classification version. A checkpoint through
  `window.storage` survives with it, deliberately and with a comment saying so, though the
  quality-of-life plan had listed that residue for removal.

## Two known defects, both still open

1. **Conversation order is unstable across restarts.** I read
   [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
   directly: `list_for_user` still iterates a `HashMap` and collects without sorting, and Rust
   seeds that hash randomly per process. The fix named in the plan (sort by a stable key) has not
   been applied. The paged detection route works around it by sorting for itself, so paging is
   stable while the list the user sees is not.
2. **Intermittent "Failed to fetch" from the page to the server, not root-caused.** Worked around
   by waiting for reachability before driving the browser. A user hitting this sees only "Is the
   backend running?" when it is.

## Places where a document claims more than the repository shows

These matter more than the list above, because they are the items most likely to be read as done.

- **The quality-of-life plan's C12 is tagged `[RESOLVED]` and I cannot find what it claims.** It
  says one command now runs every suite — the Rust workspace, the browser tests and the launcher
  test — "and that command is what 'run all tests' means", and that a check for silently swallowed
  exceptions covering both Rust and JavaScript was in scope. I searched: [scripts/](scripts/) holds
  only `dev-up.sh`, `dev-down.sh`, `port-control.sh` and `test-dev-up.sh`; there is no makefile;
  [.vscode/tasks.json](.vscode/tasks.json) has no such task; [e2e/package.json](e2e/package.json)'s
  only script is the browser test runner; and no swallowed-exception check exists anywhere in the
  repository. As far as reading can tell, that resolution is written but not built.
- **"Reset the backend before each e2e test" covers one of the two browser suites.** Commit
  `d81b89e` touched only [e2e/views.spec.js](e2e/views.spec.js).
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no per-test reset; it starts its own
  server once instead.
- **The two documents disagree on how many Rust tests there are.** [backend/README.md](backend/README.md)
  says 163; the quality-of-life plan's audit says 173 with 77.57% line coverage. Counting test
  attributes by search gives 179. I did not run the suite, so the real number is unknown — but the
  two documents cannot both be current.
- **C8 in the quality-of-life plan is still tagged `[OPEN]`** ("silent restore, or announced?")
  although its own status table says restore is announced and
  [timeline.html](timeline.html) has the notice banner at line 779. Stale tag, not open work.
- **The file-size ratchet cannot see the page.**
  [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) scans only
  Rust files under [backend/](backend/), which the plan states openly.

## The shortest summary

The page and the local server work together and are exercised by real browser tests; everything
that would make this a deployed product — the real storage adapters, the template, the real user
pool, the deployment itself, classification, payment — is written or planned but has never run
against anything real. The active task is not the product at all; it is an unrun experiment on how
[CLAUDE.md](CLAUDE.md) should word its vocabulary rule, whose instruction text is sitting
uncommitted in that file right now. Before trusting the quality-of-life plan's "all six phases
Done" table, note that one of its resolved items (a single command that runs every suite, plus a
swallowed-exception check) does not appear to exist on disk.
```

---

## A-q3-r2

**Variant on disk (not what reached the instance):** A  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work left off, and what is verified

Everything below comes from reading files, git history, and `grep`/`find` results in the working
tree. I ran no tests and changed no code, so nothing here is a fresh measurement — where a number
comes from a document rather than from something I watched happen, I say so.

## Where it left off

**Last change to the product code: 2026-09-29 14:40**, three commits landing one small feature —
`POST /_dev/reset`, a route that empties the local in-memory stores, plus a test that the route
cannot reach a deployable build:

- `da1457a` Let the in-memory stores be emptied
- `d725cc3` Add POST /_dev/reset, and test that _dev stays out of the Lambda build
- `d81b89e` Reset the backend before each e2e test
- `7cb86d9` Mark C11 resolved: the Lambda build is now tested for _dev absence

**Everything after that is writing about writing, not product work.** The four most recent commits
(2026-09-30, 03:19–03:56) are an experiment on which written instruction reduces vocabulary a reader
would not already share: [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md)
reports the first attempt, and [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
plans a second. The first attempt reached **2 completed runs per condition against a target of 10**;
20 earlier runs were launched in the background and all 20 were lost when the coordinating session
ended. The second attempt is written but I see no sign in the tree that it has been run — none of the
three files it says it will produce exist under [docs/analysis/](docs/analysis/).

**Uncommitted in the working tree** (two files, per `git status`):
- [CLAUDE.md](CLAUDE.md) — one wording fix in the workflow section, plus a new "Vocabulary" section
  at line 77 reading "Write at the level of language you would expect from someone familiar with
  object-oriented software engineering texts and practice."
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  — the three questions the experiment asks were replaced with three that can be answered by reading,
  because in the first attempt the old ones made instances run the test suite ten at a time against
  one port and write throwaway programs.

## The feature work that is finished

Six phases in [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
are all marked **Done** in its own status table: a dev-server launcher plus VS Code tasks; browser
coverage for the calendar and the five analytics views; deleting the page's own flag-detection code
and its two embedded word lists; making detection an explicit, user-triggered, progress-reporting
step; an upload progress bar; and Back/Forward navigation with session restore.

Two of those I could check against the files rather than take on the plan's word:

- **The page really did shrink.** [timeline.html](timeline.html) is now 3,110 lines / 119,073 bytes.
  The plan predicted about 112KB / 2,965 lines after the deletion, from 752,370 bytes / 66,839 lines.
- **Detection really did move.** [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)
  exists as a paged route, and [backend/timeline-api/src/processing.rs](backend/timeline-api/src/processing.rs)
  documents at line 99 that upload no longer flags anything.
- **The false privacy line is gone.** `grep` for "never uploaded" and "read locally" in
  [timeline.html](timeline.html) returns nothing; the plan flagged that copy as now untrue.

The backend's own history, from [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md):
V1 (pure logic ported out of the page) and V2 (routes, auth checking, storage behind interfaces) are
built; V2a wired [timeline.html](timeline.html) to them through local-only stand-ins for the two
Amazon-specific mechanisms (a signed upload link, and the event that would trigger processing); and a
follow-up design review, applied to code, collapsed the upload record down to a single terminal
outcome and renamed two of the storage interfaces. Its self-critique entries C11 through C14 are
marked resolved, and the renames are present in
[backend/timeline-core/src/ports/uploads.rs](backend/timeline-core/src/ports/uploads.rs) and
[backend/timeline-core/src/ports/conversations.rs](backend/timeline-core/src/ports/conversations.rs),
so those four are genuinely applied, not just claimed.

## Verified

The honest split is already written down in [backend/README.md](backend/README.md) lines 91–150, and
I am relaying it rather than re-confirming it — I did not re-run any of it. It records as **run, not
merely read**:

- The local server driven by hand with `curl`: requests with no token and with a garbage token
  rejected, an upload request returning a signed-link-shaped response, an empty conversation list for
  a new user, and a flag write followed by a read round-tripping. Captured afterwards as committed
  tests in [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs).
- Token checking in [backend/timeline-auth/](backend/timeline-auth/), including every rejection path
  that matters for security, against real signing and verification with a throwaway key.
- The in-memory storage adapters, including that automatic flags and the user's own corrections cannot
  overwrite each other, exercised through the public methods.
- A real cross-compiled ARM64 Lambda binary, and four genuine API-Gateway-shaped events sent at it
  through `cargo lambda`'s local emulation of the Lambda runtime, all answering correctly. That was a
  manual session; it is not a committed test.

The browser suite is the other half. [e2e/views.spec.js](e2e/views.spec.js) and
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) hold roughly twenty tests that drive real Chrome
against a real locally-running server. The only record of a result on disk is
`e2e/test-results/.last-run.json`, which says `"status": "passed"` with an empty failure list, last
written 2026-09-30 04:14. **Treat that as weak evidence**: the file is ignored by git, it names no
tests and no count, it records only the most recent run, and per the experiment plan the suite was
being run repeatedly and concurrently around that time by other sessions fighting over the same port.

## Not verified

- **Nothing has ever reached Amazon's services.** [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs)
  and [backend/timeline-storage/src/dynamo.rs](backend/timeline-storage/src/dynamo.rs) say so in their
  own opening comments: they compile, their request-building is unit-tested, and no call in either has
  ever reached the real service or a local emulator. [infra/template.yaml](infra/template.yaml) (193
  lines describing the bucket, three tables, the login pool, the function and the gateway) has never
  been validated or deployed. Nothing has been checked against a real login pool's tokens. This is the
  migration plan's open item C10, and it is still labelled "real, unstarted work."
- **The paid, generative classification pass (V3), payment (V4) and production hardening (V5) do not
  exist.** V3 also still waits on the user's agreement to make one real, small paid call in order to
  capture a sample request and response — the plan's C6, still open.
- **How the two kinds of detection are named for users is tabled**, by the user's own direction. The
  free keyword-and-dictionary pass and the paid generative pass currently share the interface with no
  agreed wording, which is what prompted the question in the first place.

## Five gaps I found where a document claims more than the code delivers

These are the parts I would want to know before treating any status table as current.

1. **The conversation-ordering defect is still there.** The quality-of-life plan lists it under "Known
   defects — fix before shipping": conversation order shuffles between backend restarts because the
   store returns summaries in hash-map order. Still true.
   [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
   lines 36–47 collect without sorting; [backend/timeline-api/src/routes/conversations.rs](backend/timeline-api/src/routes/conversations.rs)
   line 16 passes the result straight through; [backend/timeline-api/src/routes/export.rs](backend/timeline-api/src/routes/export.rs)
   sorts the upload identifiers at line 54 but never the summaries. The newer detection route works
   around it by sorting for itself before paging, so the underlying unordered result is now depended on
   in two places with different remedies.
2. **The second known defect is still unexplained.** Intermittent "Failed to fetch" from the page to a
   backend that was demonstrably answering. The plan says it was worked around by waiting for
   reachability, not root-caused. I found nothing suggesting that changed.
3. **The plan entry claiming one command runs every suite has not landed.** That entry (its C12) is
   marked resolved and reads as though the command exists. I searched [scripts/](scripts/),
   [.vscode/tasks.json](.vscode/tasks.json), [e2e/package.json](e2e/package.json) and
   [backend/package.json](backend/package.json): there are tasks to start, stop and test the launcher,
   and `npm test` inside [e2e/](e2e/), but nothing that runs the Rust suite, the browser suite and the
   launcher test together. The same entry promises a check for exceptions that get quietly discarded,
   covering both languages. No such check exists: there is no Python file anywhere in the repository
   and no `.github` directory, so there is also no automated run of anything.
4. **The file-size ratchet cannot see the page.** [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs)
   walks only `.rs` files under [backend/](backend/). [timeline.html](timeline.html) is 3,110 lines,
   above the 1,000-line ceiling [CLAUDE.md](CLAUDE.md) sets, and outside that test's reach. The plan
   notes this as remaining; nothing has been done about it.
5. **The real storage adapters are wired into nothing at all.** `grep` for `S3ObjectStore`,
   `DynamoConversation` and `DynamoMessageFlags` across `backend/**/*.rs`, excluding the crate that
   defines them, returns no hits. Reading [backend/timeline-api/src/main.rs](backend/timeline-api/src/main.rs),
   the Lambda branch calls the same `build_local_state()` as local development and then hands
   `build_router` the in-memory stores. So a deploy today would run against storage that is thrown away
   between calls. That is consistent with the plan's staging — the deployment step is not claimed done
   — but it is a larger gap than "the adapters are untested" suggests, and I did not see it stated
   anywhere: the adapters are unreferenced, not merely unexercised.

Two smaller ones: the reset-before-each-test step landed only in [e2e/views.spec.js](e2e/views.spec.js)
(line 128), while [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) still has no reset, so the two
files start from different assumptions about leftover state. And the test count disagrees between
documents — [backend/README.md](backend/README.md) line 66 says 163, the quality-of-life plan's audit
says 173 with 77.57% of lines covered — while three test files have been added since either number was
written. Both are stale, and I have no fresh figure because I did not run anything.

## One discrepancy in my own instructions, since it bears on what "verified" means here

The [CLAUDE.md](CLAUDE.md) supplied to me for this task contains a section titled "Only use words that
are already in the project," several hundred words long, setting a strict rule about vocabulary. That
section is **not in the file on disk** — `grep '^## '` over the working tree shows "Vocabulary" at line
77 where my copy has the other section, the committed version at `HEAD` has neither, and
`git log -S` finds the phrase in no commit that ever touched the file. So the instructions I was given
and the instructions in the repository differ on exactly the point the current experiment is measuring.
Stating it rather than quietly resolving it: I do not know which one was intended to be in force, and I
did not edit either file.
```

---

## A-q3-r3

**Variant on disk (not what reached the instance):** A  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is verified

Read-only. I ran no tests and wrote no code. Everything below is either something I read in a
file or something I worked out from what I read; I say which.

## Where work left off

Three bodies of work, newest first.

**1. An experiment about which written instruction reduces unshared vocabulary — planned, not
run.** The last four commits are all this. The first attempt is written up in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md);
its own summary says the target was ten runs per condition and
[two per condition finished (line 14)](docs/analysis/2026-09-30-vocabulary-instruction-results.md#L14).
A second design is committed at
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).
None of the three files that design says it
[will produce (line 125)](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L125)
exist yet: [docs/analysis/](docs/analysis/) holds only the first attempt's write-up. So the second
design has not been carried out, and by this project's own numbered loop in
[CLAUDE.md](CLAUDE.md) it is sitting at the step where the user says yes or no.

Two uncommitted changes are open on disk, both edits to that design and to
[CLAUDE.md](CLAUDE.md) — `git diff --stat` reports 35 added and 15 removed lines across the two.

One thing I should flag rather than pass over, because it bears directly on "what is verified".
Three copies of [CLAUDE.md](CLAUDE.md) do not agree:

- the committed copy (`git show HEAD:CLAUDE.md`) has no section about vocabulary at all;
- the copy on disk now has a `## Vocabulary` section reading "Write at the level of language you
  would expect from someone familiar with object-oriented software engineering texts and practice";
- the copy that was loaded into this session instead has a section headed "Only use words that are
  already in the project", and no `## Vocabulary` section.

The first two I read directly. The third I can see in my own instructions. Putting them together,
my reading is that the experiment swaps this file between conditions and that this session was
started from one variant while a different variant is on disk — but that is my inference from three
snapshots, not something I watched happen. The design's own
[stopping rules (line 144)](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L144)
say to restore the file and check its hash after every condition, for exactly the reason that it
carries the user's uncommitted work. Right now the on-disk copy is not the committed copy.

Also worth noting: the second design links to
`docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md` as the place where two retired
questions "are recorded". That file does not exist. The facts it cites are in the *first* attempt's
write-up. That link is pointing at a file the experiment has not written yet.

**2. Six frontend quality-of-life phases — all six marked Done.**
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
opens with a table where every row says **Done**: the launcher scripts, added browser-test cover
for the calendar and the five analysis views, deleting the page's own detection code and its two
embedded word lists, moving detection behind a user action with visible progress, an upload
progress bar, and back/forward navigation. [timeline.html](timeline.html) is now 3,110 lines /
119,073 bytes, down from the 66,839 lines the plan measured.

**3. The Rust backend migration — stopped at the point where everything runs in memory.**
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
lays out five versions. What exists is the pure logic library, the routes, and a local-dev server
holding everything in memory. Nothing beyond that: I found no reference to Bedrock or Stripe
anywhere under [backend/](backend/) (one match, in a comment in
[backend/timeline-core/src/ports/message_flags.rs](backend/timeline-core/src/ports/message_flags.rs)),
so the classification-by-inference version, the payment version and the hardening version are
unstarted. The quality-of-life plan says as much in its own first paragraph — those phases were
meant to finish *before* returning to the server work.

## What is verified

- **The Rust suites exist and are extensive.** I counted roughly 174 test attributes across the
  four crates' `tests/` directories — the largest groups being the sentiment scorer, deduplication,
  the flag rules, and session splitting. I did not run them, so I am reporting what is written, not
  a pass.
- **The `_dev` routes are kept out of a deployable build by a test, not by a comment.**
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  the same router the Lambda branch builds and asserts the `/_dev/*` paths answer 404 while the
  real routes answer "unauthorized" — so the test cannot pass against an empty router. This closed
  the plan's own concern C11, and it matters because
  [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs)
  empties every store.
- **The browser tests drive a real browser against a real server.** [e2e/](e2e/) has 17 tests
  across two files — three in
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and fourteen in
  [e2e/views.spec.js](e2e/views.spec.js) — each file starting `cargo run -p timeline-api` itself and
  opening [timeline.html](timeline.html) as a local file. They cover the calendar, a day click,
  opening a conversation, all five analysis views, review search and paging, a flag the server
  produced rendering as flagged, the annotated download, uploading *without* asking for detection
  and getting no flags, detection reporting progress, byte progress on the upload, back/forward
  through the location hash, and restoring a session on reload.
  [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json) records `"status": "passed"`
  with no failures, written 2026-09-30 04:14 — that is a record of a past run, not evidence about
  the working tree as it stands.
- **Deleting the page's detection code changed nothing on screen, and this was checked rather than
  assumed.** The commit message for `5faffce` states that both builds were captured against one
  server and produced byte-identical rendered markup for the calendar, conversation list,
  transcript, review table and paging, with all five analysis views identical and every statistic
  equal, the only difference being a modal that no longer appears.
- **Adding a reset call to the browser tests cost almost nothing, measured.** Commit `d81b89e`
  reports 49.6 seconds against a 47.6–51.8 second baseline.

## What is not verified

- **The real storage adapters have never run against anything.**
  [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs),
  [backend/timeline-storage/src/dynamo/conversations_table.rs](backend/timeline-storage/src/dynamo/conversations_table.rs)
  and
  [backend/timeline-storage/src/dynamo/message_flags_table.rs](backend/timeline-storage/src/dynamo/message_flags_table.rs)
  are constructed by nothing — searching the whole of [backend/](backend/) for their type names
  returns only their own definitions. [backend/timeline-storage/tests/](backend/timeline-storage/tests/)
  holds four files, all four for the in-memory versions. The migration plan says this plainly at
  [C10, whose mitigation line reads "none yet — this is real, unstarted work" (line 942)](docs/plans/2026-09-09-rust-aws-backend-migration.md#L942).
- **The Lambda branch would run on the in-memory stores.**
  [backend/timeline-api/src/main.rs:117-125](backend/timeline-api/src/main.rs#L117-L125) takes the
  Lambda path and calls `build_local_state()` — the same function the local path calls, which builds
  four in-memory stores. So nothing deployable is wired to S3 or DynamoDB yet. The module comment
  above it is candid that local mode "is genuinely 'run the whole app with no AWS at all'". Nothing
  under [infra/template.yaml](infra/template.yaml) (193 lines, describing a bucket, three tables, a
  user pool, a function and an interface) has any sign of having been deployed.
- **Two known defects are recorded and still present in the code.** The plan lists both at
  [its "Known defects" section (line 466)](docs/plans/2026-09-28-frontend-quality-of-life.md#L466).
  I checked the first one: `list_for_user` at
  [backend/timeline-storage/src/memory/conversations.rs:36-48](backend/timeline-storage/src/memory/conversations.rs#L36-L48)
  still walks a `HashMap` and collects without sorting, so conversation order still changes between
  server restarts. No commit since has touched it. The second — the page intermittently reporting it
  cannot reach the server while the server answers — is recorded as worked around, not explained.
- **One resolved item in the plan is not matched by the code, and this is the largest gap I found.**
  [C12 (line 444)](docs/plans/2026-09-28-frontend-quality-of-life.md#L444) is tagged `[RESOLVED]`
  and its resolution reads "one command runs every suite — the Rust workspace, the Playwright e2e
  suite, and the launcher test — and that command is what 'run all tests' means." I can find no such
  command. [scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh` and
  `test-dev-up.sh`; [e2e/package.json](e2e/package.json) has one script, `playwright test`;
  [backend/package.json](backend/package.json) has no scripts at all; there is no `.github`
  directory. The same block also commits, "per the user's direction", to a check for swallowed
  errors across both the Rust and the JavaScript, noting it has to be written from scratch — and
  [there is no such file](tests/) anywhere in the repository. So the three suites still have to be
  remembered and run separately, which is the thing C12 says was the defect. I would treat that
  entry as not actually resolved.
- **Line coverage has not been re-measured since the page was cut down and detection moved.** The
  audit figure quoted in the plan is 77.57% from `cargo llvm-cov`, taken before the phases landed.
  [backend/README.md:66](backend/README.md#L66) says the workspace has 163 tests; the plan's audit
  says 173; I counted about 174 test attributes. I did not run anything, so I cannot say which is
  current — only that the two written numbers disagree with each other, and with my count.
- **The reset-before-every-test change reached only one of the two browser-test files.** Commit
  `d81b89e` is titled "Reset the backend before each e2e test" and touched
  [e2e/views.spec.js](e2e/views.spec.js#L128) alone.
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no call to `/_dev/reset`; its three tests
  share one server and whatever earlier tests left in it. Each file does start its own server, so
  the file as a whole begins clean — but the commit title claims more than what landed.
- **Some of the holes the plan's own audit listed are still open.** The query-parameter branch of
  `resolveApiBase` ([timeline.html:1045](timeline.html#L1045)) — the one a remote setup depends on —
  has no test; searching [e2e/](e2e/) for `api_base` returns nothing. `classifyWithAI`
  ([timeline.html:1809](timeline.html#L1809)) is still in the file, still untested, and the plan
  suggested deciding whether to delete it rather than test it; that decision has not been made.
  `describeFailure` ([timeline.html:1087](timeline.html#L1087)) is untested.
- **The file-size check only looks at Rust.**
  [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) scans `.rs`
  files under [backend/](backend/) with an empty exemption list. The plan notes at
  [line 456](docs/plans/2026-09-28-frontend-quality-of-life.md#L456) that
  [timeline.html](timeline.html) is outside its reach and suggested adding a JavaScript arm once the
  file was small enough — the file is now 3,110 lines, so the condition is met and the arm is not
  there.
- **Two design questions are still open, and both need the user.**
  [C1 (line 364)](docs/plans/2026-09-28-frontend-quality-of-life.md#L364): parsing on the page still
  cannot show real progress, and the plan says to revisit only after measuring whether the page
  still visibly stops — that measurement has not been taken.
  [C8 (line 402)](docs/plans/2026-09-28-frontend-quality-of-life.md#L402): restoring on load was
  built and announced, per the user's call, so this one is effectively answered in the code. And the
  plan's [own tabled item (line 487)](docs/plans/2026-09-28-frontend-quality-of-life.md#L487) —
  how the two kinds of emotion detection are named for users — is the user's to design; the code
  deliberately invents no labels for it.
- **Documentation has drifted in two places.** [e2e/README.md:38](e2e/README.md#L38) lists only
  `upload-flow.spec.js` under what is covered — the fourteen tests in
  [e2e/views.spec.js](e2e/views.spec.js) are missing. [backend/README.md:66](backend/README.md#L66)
  carries the stale test count noted above.

## The shortest honest summary

The local version works and is well covered by tests that drive it for real; the deployable version
does not exist yet, because nothing is wired to real storage and the real storage code has never
been executed. Of the things marked finished, the one I would not accept as finished is the single
command that runs every suite, together with the promised check for swallowed errors — both are
written down as resolved and neither is in the repository. The two recorded defects are still in the
code. And the most recent work, the writing-instruction experiment, is a committed design that has
not been run, with the user's own `CLAUDE.md` currently holding an uncommitted variant of itself.
```

---

## C-q1-r1

**Variant on disk (not what reached the instance):** C  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and both standard answers are already in this repository — one of them is in use, the other
covers the tests that can't use it. Everything below is read in source; I ran nothing.

## The two standard answers

**1. Give each test its own instance, so there is nothing shared to reset.** The backend tests
already do this: every one builds its own set of routes over brand-new in-memory stores, e.g.
[backend/timeline-api/tests/detect.rs:38](backend/timeline-api/tests/detect.rs#L38) and
[backend/timeline-api/tests/dev_routes.rs:34](backend/timeline-api/tests/dev_routes.rs#L34). Nothing
carries over between them because nothing is shared in the first place. Where this is available it
is the stronger of the two, because a test can't forget to do it.

**2. For tests that must share one long-lived server, run one emptying step before every test.**
That is what the browser tests under [e2e/](e2e/) need, and the machinery exists:

- `POST /_dev/reset` ([backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs))
  empties the stores it is handed.
- The one thing it needs from a store is `reset()`
  ([backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24)),
  which is deliberately not one of the storage interfaces the rest of the code programs against, and
  is implemented only by the in-memory stand-ins — so there is no path by which it could erase a
  real S3 bucket or DynamoDB table.
- The four stores it covers are listed at
  [backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92): stored objects,
  conversation summaries, message flags, upload outcomes.
- Playwright's standard place to call it is a step declared to run before each test —
  `test.beforeEach` — which is exactly what
  [e2e/views.spec.js:128](e2e/views.spec.js#L128) does, through the helper at
  [e2e/views.spec.js:50](e2e/views.spec.js#L50).

So for the question as asked: `test.beforeEach` plus a local-dev-only "empty everything" route is
the standard shape, and it is already built and working in one of the two test files.

Two related points:

- **The browser side needs nothing extra.** Playwright starts each test with its own fresh browser
  state, so the two values the page keeps in the browser — the API address and the dev login name,
  written at [timeline.html:1049](timeline.html#L1049) and
  [timeline.html:1075](timeline.html#L1075) — begin empty in every test regardless. The only state
  that survives from test to test is the server's.
- **Giving each test its own user id is not a substitute**, and the code comments already say why
  ([backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs)):
  distinct ids avoid collisions but leave every earlier test's data sitting in the store, where
  anything not keyed by user still finds it. Both are worth keeping — the distinct name makes a
  failure message point at the test that produced the data — but only the emptying step gives a
  known starting state.

## Three places the pattern isn't applied yet (read, not run)

**a. [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets.** It has no before-each step
at all. Both test files talk to the same server on port 3000 and run one at a time
([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)), so whichever file runs second starts
on whatever the first left behind. Same file also starts a server unconditionally
([e2e/upload-flow.spec.js:61](e2e/upload-flow.spec.js#L61)) with no check for one already listening,
and stops it by signalling `cargo` alone
([e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77)) rather than the whole group of processes
`cargo` started — which is the case
[e2e/views.spec.js:109-114](e2e/views.spec.js#L109-L114) and
[e2e/views.spec.js:136](e2e/views.spec.js#L136) were written to handle. A server still holding port
3000 after the run ends is a non-neutral starting state for the *next whole run*, not just the next
test.

**b. The start/wait/stop/reset code is copied between the two files.** The standard way to share a
before-each step across Playwright files is to build an extended `test` in one shared module with
`test.extend`, declaring the reset as setup that runs automatically, and have both files import
`test` from that module instead of from `@playwright/test`. The config's `globalSetup` is the wrong
place for this — it runs once per run, not before each test.

**c. Resetting port 3000 can erase a real session.** [scripts/dev-up.sh:9-10](scripts/dev-up.sh#L9-L10)
exists specifically because timeline-api holds every upload and confirmed flag in memory, so it
restarts only when something actually changed; and the launcher's own test stays off 3000/8000 for
the same reason ([scripts/test-dev-up.sh:10](scripts/test-dev-up.sh#L10)). The browser tests hardcode
3000 ([e2e/views.spec.js:22](e2e/views.spec.js#L22),
[e2e/upload-flow.spec.js:17](e2e/upload-flow.spec.js#L17)), reuse a server already listening there,
and now empty it before every test — so running them while you have a dev server up with a
conversation loaded will throw that conversation away. The fix is the same one the launcher's test
already chose: read the port from the environment, default to something else (3999/8999 are already
taken by the launcher's test, so e.g. 3100/8100), and always start your own server there. That also
removes the reuse-or-spawn ambiguity in a; each run then begins neutral by construction, which is
answer 1 applied at the level of the run.

## When the real AWS stores get tested

`reset()` is intentionally unavailable there, so this route cannot be pointed at real data. The
usual approach for that case is a distinct table name or key prefix per run plus deletion at the
end, since DynamoDB has no undo-a-batch-of-writes step to lean on. That belongs with the plan's
still-open entry on testing the real S3/DynamoDB adapters
([docs/plans/2026-09-09-rust-aws-backend-migration.md:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926)),
not with the in-memory work.

## What I'd want your decision on

Items a-c are three small changes to files under [e2e/](e2e/) and would need a plan in
[docs/plans/](docs/plans/) before I write any of it. Say the word and I'll write that plan; c is the
one I'd rank first, because it can destroy work you have open rather than just make a test flaky.
```

---

## C-q1-r2

**Variant on disk (not what reached the instance):** C  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and most of it is already built here — but it is only applied to one of the two browser test
files, which I think is the real finding.

## What already exists

- `POST /_dev/reset` — [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs).
  It walks a list of stores and empties each one, returning 204.
- The one thing a store has to be able to do for that to work:
  [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24).
  Only the in-memory stores implement it, so there is no reachable path from this route to a real
  S3 bucket or DynamoDB table, and it lives in the `_dev` router, which
  [backend/timeline-api/src/main.rs](backend/timeline-api/src/main.rs) merges in only outside Lambda.
- All four stores are in the list —
  [backend/timeline-api/src/main.rs:92-97](backend/timeline-api/src/main.rs#L92-L97) names the object
  store, the conversation summaries, the flags and the upload outcomes. So the reset is complete, not
  partial. (Read in source; I did not run it.)
- The browser tests in [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) call it from
  `test.beforeEach`, which is Playwright's built-in "run this before every test in this file".
  [commit d81b89e](docs/plans/) records that this replaced giving each test its own login name, and
  records the cost: the suite ran in 49.6s against a 47.6-51.8s range before.

So the shape you are describing is the one this project already chose, and the reasoning is written
down in the module comment at
[backend/timeline-storage/src/memory/resettable.rs:10-14](backend/timeline-storage/src/memory/resettable.rs#L10-L14):
a separate login name per test avoids collisions but leaves the earlier data sitting there for
anything not keyed by user to find.

## The gap

[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never calls the reset route. It has a
`beforeAll` and an `afterAll` and no per-test step at all
([e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53),
[e2e/upload-flow.spec.js:76](e2e/upload-flow.spec.js#L76)). It also never fills `#devLoginSub`, so
all three of its tests run as `alice`, the value
[timeline.html:752](timeline.html#L752) pre-fills into that box. Its three tests therefore pile up in
one user's data: the second uploads 4000 messages, and the third then opens the review table and
clicks `.approve-btn` *first*
([e2e/upload-flow.spec.js:194](e2e/upload-flow.spec.js#L194) region). Which row that is depends on
what the earlier two tests left behind. The other assertions in that file survive the pile-up because
they ask whether text is *contained* and whether a count is *above zero*, not what the exact state is.

I have not run the suite (you asked me not to), so I cannot tell you whether this file is failing
today. What I can say from reading is that it has no protection against the exact problem the other
file solved, and that its passing depends on assertions loose enough not to notice.

## The standard way to apply it once, for both files

Two mechanisms, both already in the installed Playwright (1.63.0):

1. **A shared setup entry, applied automatically.** `test.extend` in one small file, with the entry
   marked `{ auto: true }` — confirmed present in the installed type definitions at
   [e2e/node_modules/playwright/types/test.d.ts:6872](e2e/node_modules/playwright/types/test.d.ts#L6872).
   Both spec files then require that file instead of `@playwright/test`, and the reset happens for
   every test in every file without either file having to remember. Playwright's own name for these
   entries is the same word this repo already uses for the sample data file
   ([backend/timeline-core/tests/fixtures/sample_conversations.json](backend/timeline-core/tests/fixtures/sample_conversations.json)),
   so I would not reuse that word — call the file something like `e2e/timeline-test.js`.
   One thing to check before relying on it: I believe the automatic entry is set up before a file's
   own `beforeEach` runs, but I have not verified that ordering, and the fix for
   [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) does not depend on it either way.

2. **`webServer` in [e2e/playwright.config.js](e2e/playwright.config.js)** —
   `{ command, url, reuseExistingServer }`, at
   [e2e/node_modules/playwright/types/test.d.ts:1049](e2e/node_modules/playwright/types/test.d.ts#L1049)
   and [:11030](e2e/node_modules/playwright/types/test.d.ts#L11030). This is the built-in version of
   what both spec files currently hand-roll: each one has its own copy of `waitForPort` and its own
   `spawn` of `cargo run`, and **the two copies disagree**.
   [e2e/views.spec.js:90-118](e2e/views.spec.js#L90-L118) reuses a server that is already listening,
   spawns detached, and kills the whole process group;
   [e2e/upload-flow.spec.js:53-78](e2e/upload-flow.spec.js#L53-L78) spawns unconditionally and sends
   SIGTERM to the one process it spawned, which is `cargo`, not the server binary. If `cargo run`
   does not pass that signal on, the server survives holding port 3000, the next file finds it
   listening and reuses it, and the suite quietly tests whatever binary that older process is
   running. I have not verified whether `cargo` forwards the signal — that is the one thing to check,
   and `webServer`'s shutdown option signals the process group, which sidesteps the question.
   [scripts/dev-up.sh](scripts/dev-up.sh) already solves the same "is the thing listening current?"
   problem for hand-driven work, so there is prior art here to copy the intent from.

## Two places that need nothing

- **The Rust tests.** Each one builds its own router over its own fresh stores — see `test_router` in
  [backend/timeline-api/tests/dev_routes.rs:33](backend/timeline-api/tests/dev_routes.rs#L33). That is
  the other standard answer to your question, a new instance per test, and it is already correct.
  That file passes an explicitly empty reset list, with a comment saying why
  ([backend/timeline-api/tests/dev_routes.rs:60-64](backend/timeline-api/tests/dev_routes.rs#L60-L64)).
- **The browser's own stored values.** [timeline.html](timeline.html) keeps two things in
  localStorage — the API address ([timeline.html:1049](timeline.html#L1049)) and the dev login name
  ([timeline.html:1075](timeline.html#L1075)) — and restores the login name on load
  ([timeline.html:1264-1267](timeline.html#L1264-L1267)). Playwright gives every test a new browser
  context, so those start empty without anyone doing anything. Worth knowing they exist, because if
  tests ever share one page they become leftover state too.

## What I would do

1. Move the reset call and the server start-up out of both spec files into one shared file, applied
   automatically, and delete the two divergent copies.
2. While doing it, decide what `#devLoginSub` should be in
   [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) — a distinct name per test, as
   [e2e/views.spec.js:59](e2e/views.spec.js#L59) does, for readable failure messages, not for
   separation, which the reset now provides.
3. Then tighten the assertions in that file that currently pass regardless of leftover data, since
   only after a reset can they say what the state actually is.
```

---

## C-q1-r3

**Variant on disk (not what reached the instance):** C  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and you already have the hard part. `POST /_dev/reset` ([backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24)) is the mechanism; what is missing is the standard *plumbing* that makes it run for every test without anyone remembering it. Two standard pieces, both present in the Playwright already installed here (1.63.0, per [e2e/package.json](e2e/package.json) and the installed `playwright/package.json`).

1) Put the reset in a shared setup value, not a per-file `beforeEach`

Playwright's own mechanism for "every test gets this, automatically" is `test.extend` with the `auto: true` option — a setup step attached to the test object itself, which runs for every test in every file that imports that test object, whether or not the test mentions it. I read the option in the installed type declarations: `e2e/node_modules/playwright/types/test.d.ts:6872` shows `{ scope?: 'test', auto?: boolean, ... }` for per-test values, and line 6870 the same for per-worker ones.

Why this matters more than style here: the reset is currently a `test.beforeEach` inside one file, [e2e/views.spec.js:128](e2e/views.spec.js#L128), calling `resetBackend` at [e2e/views.spec.js:50](e2e/views.spec.js#L50). I grepped [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) for `reset` and `beforeEach` and both return nothing — so the other half of the suite still starts from whatever the previous file left in the stores. A per-file hook is exactly the thing that gets forgotten when a third spec file is added; an automatic setup value on a shared test object cannot be.

Shape: one new small file (say `e2e/backend.js`) exporting `const test = base.extend({ cleanBackend: [async ({}, use) => { await resetBackend(); await use(); }, { auto: true }] })` plus the single `API_BASE` constant that both specs currently duplicate at [e2e/views.spec.js:22](e2e/views.spec.js#L22) and [e2e/upload-flow.spec.js:17](e2e/upload-flow.spec.js#L17). Each spec then does `const { test, expect } = require('./backend')` instead of requiring `@playwright/test` directly.

One trap when moving the call: reset must stay per *test*, not per page load. The third test in [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) confirms a flag, then re-uploads the same file inside the same test and asserts the override comes back — it depends on the backend keeping state across two loads. Putting the reset inside `loadFixture`/`loadFixtureAndWaitForRender` would break it.

2) Hand the server lifecycle to the config's `webServer`, not to `beforeAll` in each file

Playwright's standard answer for "a real server must be up, and the suite must not inherit a dirty or foreign one" is the `webServer` entry in the config. I read its fields in the installed types (`types/test.d.ts:10979-11060`): `command`, `cwd`, `env`, `port`/`url`, `reuseExistingServer`, `gracefulShutdown: { signal, timeout }`, `stdout`/`stderr`, and `wait: { stdout: RegExp }` (start is considered done when a regex matches the server's own output — your server prints `timeline-api (local dev, in-memory storage) listening on ...`).

Adopting it deletes, rather than adds, code: both copies of `waitForPort` ([e2e/views.spec.js:29](e2e/views.spec.js#L29), [e2e/upload-flow.spec.js:22](e2e/upload-flow.spec.js#L22)), both `spawn` blocks, the `startedServerHere` flag ([e2e/views.spec.js:88](e2e/views.spec.js#L88)), the hand-rolled reuse probe at [e2e/views.spec.js:90](e2e/views.spec.js#L90) (that is what `reuseExistingServer` means), and both teardown blocks. The `PATH` fix-up for cargo/zig moves to `webServer.env`.

It also closes a live hole in the "neutral start" story. [e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53) spawns unconditionally with no reuse probe, not detached, and tears down with `serverProcess.kill('SIGTERM')` at [e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77) — that signal goes to `cargo`, and `cargo run`'s child is the actual server. [e2e/views.spec.js:133](e2e/views.spec.js#L133) already documents why it spawns detached and kills the whole group instead. So the plausible failure (I have read this in the source, not observed it running — you asked me not to run the tests) is: `upload-flow` starts a server, exits leaving the real server holding port 3000, `views` finds it listening and reuses it. Two files, two different assumptions about who owns the process. `webServer`'s documented default is to `SIGKILL` the process *group*, with `gracefulShutdown` if you want SIGTERM first.

3) Browser-side state needs nothing — the standard mechanism already covers it

[timeline.html](timeline.html) remembers state in `localStorage`: the API base at line 1049, the dev login name at line 1075, and restores the login name at line 1264. That does not leak between tests, because Playwright's per-test `page` comes from a fresh browser context and the config sets no `storageState` ([e2e/playwright.config.js](e2e/playwright.config.js)). So do not hand-roll storage clearing; it would be the wrong instinct copied from the server-side problem. The only thing that would change this is deliberately sharing a context or setting `storageState`.

4) Worth deciding now: reset plus a reused server can wipe a real session

[scripts/dev-up.sh](scripts/dev-up.sh) exists specifically to avoid restarting `timeline-api` when nothing changed, because "restarting it when nothing changed throws away whatever you were looking at" (its header comment, lines 9-13). The browser tests hardcode port 3000 and [e2e/views.spec.js:90](e2e/views.spec.js#L90) deliberately reuses whatever is listening there — then empties it before every test. So running the suite while you have a dev session up silently destroys that session. Note that `dev-up.sh`'s comment about the test suite using "its own ports" refers to [scripts/test-dev-up.sh](scripts/test-dev-up.sh), which sets `PORT=3999` (line 21); it does not describe the Playwright suite.

The standard fix pairs with piece 2: give the browser tests their own port via `webServer.env` (`PORT: '3100'`) and the single shared `API_BASE`. Then `reuseExistingServer` can only ever reuse a test server, and "reset before every test" stops being destructive to anything a human cares about. This is the part I would want your call on, since it changes which server the suite talks to.

Scope of what reset actually resets (so the guarantee is not overstated)

[backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92) registers four stores: the object store, conversation summaries, message flags, upload outcomes. Those are all four in-memory stores in [backend/timeline-storage/src/memory](backend/timeline-storage/src/memory), so the list is complete today. Nothing ties the two together, though: a fifth store added later would be missed silently, and the existing test [backend/timeline-api/tests/lambda_router.rs:104](backend/timeline-api/tests/lambda_router.rs#L104) (`reset_empties_the_stores_it_is_given`) checks only the ones the test itself hands over. `build_local_state` is private to `main.rs`, so a test over the public interface cannot inspect its registration list as things stand — I am flagging the gap rather than proposing to reach into it.

The dev signing keypair is process-wide and deliberately not reset ([backend/timeline-api/src/dev_only.rs:28](backend/timeline-api/src/dev_only.rs#L28)); tokens minted before a reset stay valid, which is what lets a test log in once and reset repeatedly.

The Rust integration tests do not need any of this: [backend/timeline-api/tests/dev_routes.rs](backend/timeline-api/tests/dev_routes.rs), [detect.rs](backend/timeline-api/tests/detect.rs) and [export.rs](backend/timeline-api/tests/export.rs) each build their own router and stores and pass `resettable: Arc::new(vec![])` with a comment saying so, so every one of them already starts empty by construction. The shared-state problem is confined to the browser tests, where one server process outlives many tests.

Alternatives I considered and would not use here: `globalSetup` (runs once per suite — wrong granularity for per-test reset, and `webServer` covers the once-per-suite need); keeping isolation by unique user id alone (already rejected in [backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs)'s module comment, for the right reason — it avoids collisions without giving a clean slate). Keep the unique names for what [e2e/views.spec.js:57](e2e/views.spec.js#L57) says they are now: making a failure message point at the test that produced the data.

Cost, honestly: the four items above are a small refactor of the two spec files plus the config, not a rewrite, and items 1 and 2 are pure deletion-plus-move. Item 4 is a behavior decision. I have not run anything, so none of this is verified end to end — it is all read from the source and the installed Playwright type declarations.
```

---

## C-q2-r1

**Variant on disk (not what reached the instance):** C  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
## Short answer

In the code as it stands today, it doesn't. The upload no longer scans anything. The scan is a
separate step that runs only if you tick a box on the load screen, and the box starts unticked. So
the thing you're asking about was real and has since been changed; what's left is the naming
problem you predicted, which is still there.

I read this, I did not run it. Everything below is a file and line you can open.

## Why it used to scan on every upload

[docs/plans/2026-09-09-rust-aws-backend-migration.md:32-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32-L33)
asked for a free tier that is "always available", against one paid pass per $5. What got built read
that as "already computed for everyone at upload time, whether or not anyone asked." Those are two
different things, and the second one is what you saw.

## What it does now

[backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98) says in so
many words: "**This does not compute flags.**" `process_upload` reads the file, parses it, removes
repeats, writes one summary per conversation, records that the upload is ready, and stops.

The scan lives at [backend/timeline-api/src/routes/detect.rs:72](backend/timeline-api/src/routes/detect.rs#L72),
reachable at `POST /detect`
([backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27)). The page asks for it
only when the box at [timeline.html:756](timeline.html#L756) is ticked, and it is not ticked by
default. The loop that drives it is
[timeline.html:1303](timeline.html#L1303) — it asks for five conversations at a time so the bar on
the load screen can move by a real amount instead of spinning.

So a freshly uploaded export has no automatic flags at all until you ask for them. The calendar,
the conversation list and the sessions never needed the flags, so the first view is still complete.

## Is it different from "Classify with AI"? Yes, in six ways

| | the tick box on the load screen | the button at [timeline.html:844](timeline.html#L844) |
|---|---|---|
| Runs where | on the server, in Rust | in your browser tab |
| How it decides | fixed word lists, a dictionary check that skips IRS/DARPA, and a word-score sum | sends the message text to Claude (Sonnet, [timeline.html:1691](timeline.html#L1691)) in groups of 30 and asks it |
| What it reads | your message only | your message plus the Claude reply just above it |
| What it sets | ALL CAPS, critical, angry | critical and angry only — ALL CAPS is left as it was |
| Costs money | no | yes, it is a real paid call per group |
| Where the result is kept | written to the server per message ([detect.rs:128](backend/timeline-api/src/routes/detect.rs#L128)) | only in the open tab and in the file you download; reload the page and it is gone, which [timeline.html:1590-1599](timeline.html#L1590-L1599) admits |

Two things they share, and these are the reason they read as the same feature:

- **They write to the same slot.** Each message carries two separate sets of flags: the automatic
  one and the one you confirmed yourself. Both of these write only the automatic set; neither can
  touch what you ticked ([timeline.html:987-1005](timeline.html#L987-L1005)).
- **The button overwrites the box's work.** If you tick the box and later press the button, critical
  and angry are replaced by Claude's answer ([timeline.html:1847-1851](timeline.html#L1847-L1851)).
  ALL CAPS keeps whatever the scan said. Nothing in the interface tells you that.

## What will still confuse people, and I think you are right about it

1. **Neither name says what it is.** The box says "scan for likely ALL-CAPS emphasis, criticism of
   Claude, and anger" and then, at [timeline.html:762](timeline.html#L762), explains itself with
   "keyword and sentiment heuristics" — words the reader has no use for. The button says "Classify
   with AI". A reader sees one thing that sounds like it uses Claude and one that doesn't, with no
   statement anywhere that both produce the same three tags in the same place. The frontend plan
   left the naming open on purpose — see "What this phase does NOT decide" in
   [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
   under Phase 4 — so this is a decision that has been put off, not one that has been made.
2. **The only place the two are told apart is a three-letter mark.** The Review table prints `auto`
   or `AI` next to each checkbox, or `you` if you ticked it
   ([timeline.html:2479](timeline.html#L2479)). The longer wording exists only as hover text
   ([timeline.html:2462](timeline.html#L2462)), so it is invisible on a touch screen and absent when
   the "show your tags" box is off.
3. **The two are far apart.** One is on the load screen before anything renders, the other is in the
   Review tab afterwards. Nothing on either screen mentions the other.
4. **One of them quietly does not survive.** The server keeps what the box produced; it never learns
   what the button produced. The server's own download therefore stamps every automatic flag as
   coming from the word lists, hard-coded, at
   [backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99) —
   true of what the server holds, and wrong the moment a reader assumes that download is everything
   they did. The page's own "Download annotated conversations.json" does carry the difference
   ([timeline.html:1928](timeline.html#L1928)).

## One thing I found on the way that is unrelated but worse

[timeline.html:743](timeline.html#L743) still tells the reader "nothing is uploaded anywhere." The
file is sent to the server — that is what `process_upload` reads. That line is false as written, it
is the first thing anyone reads, and it was already listed as needing correction in Phase 3 of
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md).
It has not been corrected.

## What I did not check

I read source only, as asked. I did not start the server or the page, so I cannot say that ticking
the box works end to end right now — only that the route, the loop and the box are all present and
connected in the source, and that two browser tests refer to the box
([e2e/views.spec.js:79](e2e/views.spec.js#L79), [e2e/views.spec.js:339](e2e/views.spec.js#L339)).
I also did not measure how long the scan takes; nothing I read claims to know.
```

---

## C-q2-r2

**Variant on disk (not what reached the instance):** C  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Everything below is from reading the code as it stands right now. I did not run the backend, the
page, or the tests, so nothing here is a runtime measurement.

## It no longer always does

The premise held earlier and does not hold in the current code. What I read:

- The scan runs only if you tick a box on the load screen, which starts unticked:
  [timeline.html:754-766](timeline.html#L754-L766), the box itself at
  [timeline.html:756](timeline.html#L756).
- [`handleLoadClick`](timeline.html#L1329) reads that box at
  [timeline.html:1334](timeline.html#L1334) and calls
  [`runDetectionPass`](timeline.html#L1303) only inside `if(runDetection)` at
  [timeline.html:1384-1388](timeline.html#L1384-L1388).
- `runDetectionPass` posts to `/detect` one page of conversations at a time
  ([timeline.html:1309-1328](timeline.html#L1309-L1328)) and moves the bar on the load screen after
  each page, so you can watch it.
- Server side, upload writes one summary per conversation and no flags at all. Its own doc comment
  says so: "**This does not compute flags.**"
  ([backend/timeline-api/src/processing.rs:98-105](backend/timeline-api/src/processing.rs#L98-L105)).
- The only thing on the server that writes automatic flags is the `/detect` handler
  ([backend/timeline-api/src/routes/detect.rs:126-131](backend/timeline-api/src/routes/detect.rs#L126-L131)),
  registered at [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27). The flag
  route that the Review tab writes to can only write *your* values — the handler has no automatic
  writer in scope
  ([backend/timeline-api/src/routes/flags.rs:33-41](backend/timeline-api/src/routes/flags.rs#L33-L41)).
- There are tests that assert an upload has no automatic flags until detection is asked for
  ([backend/timeline-api/tests/detect.rs:166](backend/timeline-api/tests/detect.rs#L166)) and that
  the pass computes the real values rather than a stand-in
  ([backend/timeline-api/tests/detect.rs:183](backend/timeline-api/tests/detect.rs#L183)). I read
  them; I did not run them, so "the tests exist and say this" is what I can claim, not "this passes
  today".

## Why it used to

Not an accident. The confirmed decision at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:31-34](docs/plans/2026-09-09-rust-aws-backend-migration.md#L31-L34)
gives the free tier — dictionary ALL-CAPS plus keyword and sentiment criticism-anger, near zero cost
per run — as "always available", against one paid pass per $5. What got built read "always
available" as "already computed at upload, whether or not anyone asked". Those are two different
claims, and
[docs/plans/2026-09-28-frontend-quality-of-life.md:196-263](docs/plans/2026-09-28-frontend-quality-of-life.md#L196-L263)
is the phase that separated them, written against this exact question.

## Yes, it is a different thing from "Classify with AI"

Six differences I can point at in the code:

1. **Where the work happens.** The tick-box pass is Rust on the server
   ([`heuristic_flags`](backend/timeline-api/src/processing.rs#L84)). "Classify with AI" is the page
   calling `api.anthropic.com` itself from your browser
   ([timeline.html:1746-1755](timeline.html#L1746-L1755)).
2. **How it decides.** The server pass checks words: a dictionary check for ALL-CAPS so acronyms
   don't count ([backend/timeline-core/src/flags/caps.rs](backend/timeline-core/src/flags/caps.rs)),
   a wide keyword net for criticism
   ([backend/timeline-core/src/flags/criticism.rs](backend/timeline-core/src/flags/criticism.rs)),
   and a sentiment score for anger
   ([backend/timeline-core/src/flags/anger.rs](backend/timeline-core/src/flags/anger.rs)). The
   button asks Claude Sonnet to read each message and judge it
   ([timeline.html:1691](timeline.html#L1691), prompt at
   [timeline.html:1697-1740](timeline.html#L1697-L1740)).
3. **What each one can set.** The server pass sets all three flags. The button sets only critical
   and angry ([timeline.html:1848-1851](timeline.html#L1848-L1851)) — it never touches ALL-CAPS, so
   if you have only ever pressed the button, the ALL-CAPS column is empty for a reason that nothing
   on screen explains.
4. **What it reads.** The button sends each message with up to 300 characters of the Claude reply
   before it ([timeline.html:1717-1724](timeline.html#L1717-L1724)), because you can't tell whether
   someone is criticising a reply without the reply. The server pass sees the message text alone.
5. **What it costs and where it works.** The server pass costs nothing extra. The button spends real
   tokens (the earlier estimate, about $3 at Sonnet for roughly 2,200 messages, is recorded at
   [timeline-project-decisions.md:288-289](timeline-project-decisions.md#L288-L289)) and only works
   while this page is running as a live Claude artifact — a copy of the file opened from disk has no
   route to the API at all, recorded as a permanent boundary at
   [timeline-project-decisions.md:388-393](timeline-project-decisions.md#L388-L393) and handled as a
   per-batch failure at [timeline.html:1758-1764](timeline.html#L1758-L1764).
6. **Whether the result survives.** The server pass writes into the stored flags and comes back
   embedded in every later export
   ([backend/timeline-api/src/routes/export.rs:88-104](backend/timeline-api/src/routes/export.rs#L88-L104)).
   The button's results are only ever held in the open tab, plus a checkpoint that the comment at
   [timeline.html:1592-1600](timeline.html#L1592-L1600) says is no longer read back on the next
   load. Nothing sends them to the server.

## So the confusion is real, and here is its sharpest edge

The two write the same slot. Both set the *automatic* value and both leave anything you confirmed
yourself alone — the button's own confirmation box says exactly that
([timeline.html:1817-1820](timeline.html#L1817-L1820)). That means the later one wins, and since the
button's results never reach the server while the tick-box pass does, the asymmetry bites:

- Press the button, then reload or load the file again, and you are back to the word-based values.
  The page loads its flags from what the server returns
  ([timeline.html:1391-1402](timeline.html#L1391-L1402), read at
  [timeline.html:987-1006](timeline.html#L987-L1006)), and the server has only ever stored what
  `/detect` wrote. I read the path; I did not watch it happen.
- The server's export stamps `"source": "heuristic"` on every record it writes, unconditionally
  ([backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)).
  So the label telling you which pass produced a flag is correct for one pass and cannot currently
  be wrong for the other, because the other's values never get that far.
- The per-row marker that would tell them apart — "AI" against "auto", with a hover title spelling
  out which — exists in one of the four viewing states only
  ([timeline.html:2473-2481](timeline.html#L2473-L2481)). With "Show your tags" off, the box is
  disabled and carries no label at all
  ([timeline.html:2463-2467](timeline.html#L2463-L2467)), so in that view nothing distinguishes the
  two.
- The wording points at the wrong place. The box in the Review tab says it is better than "the
  keyword/sentiment heuristic below"
  ([timeline.html:836-841](timeline.html#L836-L841)), but the thing it is comparing itself to is
  not below any more — it is a tick box on the load screen, described there in different words that
  never mention this button ([timeline.html:757-765](timeline.html#L757-L765)). Neither side names
  the other, and one side leans on a word — "heuristic" — that a user has no reason to hold.

Two more things I should not leave out. First, the naming of the two passes is the part you tabled:
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md#L249-L257)
records that the phase deliberately invented no user-facing labels and left the two triggers
unpaired, pending your own design. So the "it will confuse users" half of your question is open by
agreement, not answered. Second, in the same load screen as that tick box, the text at
[timeline.html:743](timeline.html#L743) still tells the user "nothing is uploaded anywhere", which
the upload a few lines later contradicts. That is outside your question, but it is wrong on the same
screen.
```

---

## C-q2-r3

**Variant on disk (not what reached the instance):** C  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
# Why does the backend always detect, and is that the same as "Classify with AI"?

## Short answer

It no longer does, and no, it is not the same thing. The behaviour you are describing was real and
was wrong; it was changed in commit 6c7d9a9 ("Move detection out of upload into a user-triggered
POST /detect"). The two are separate things that currently look far too alike in the interface,
which is the part still outstanding.

## Why it used to do it on every upload

The reason was a decision in [docs/plans/2026-09-09-rust-aws-backend-migration.md:30-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L30-L33):
the word-list-and-sentiment scan is the free part of the product, "~$0 marginal cost", "always
available", as against one paid pass per $5. What got built read "always available" as "already
computed before anyone asks for it". Those are different claims, and the second one is the one you
objected to. It is written up as [C10 in the quality-of-life plan (line 414)](docs/plans/2026-09-28-frontend-quality-of-life.md#L414)
and the change is [Phase 4 of the same plan (line 196)](docs/plans/2026-09-28-frontend-quality-of-life.md#L196),
whose heading is your question.

The speed argument for computing it eagerly was never a good one and the plan says so at
[line 209](docs/plans/2026-09-28-frontend-quality-of-life.md#L209): nobody has measured the pass, so
nobody can call it cheap. The reasons for moving it were that you should not be charged work you did
not ask for, and that you should be able to see it happening.

## What happens now — read in the code

- Upload parses, removes duplicates, stores, and answers. It writes no automatic flags at all.
  [backend/timeline-api/src/processing.rs:81-90](backend/timeline-api/src/processing.rs#L81-L90)
  says of the scanning function: "Shared with `routes::detect`, which is the only thing that runs it
  now." I checked every caller of `set_auto_flags` in the sources and `routes::detect` is the only
  one, so this is observed, not taken from the comment.
- The scan has its own route, [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs),
  whose opening comment states the same thing: detection "deliberately does not happen at upload
  time ... it runs only when the user asks for it".
- The asking is a box on the load screen, unticked, at
  [timeline.html:756](timeline.html#L756): "After uploading, scan for likely ALL-CAPS emphasis,
  criticism of Claude, and anger. This runs only if you tick it." The page reads it at
  [timeline.html:1334](timeline.html#L1334) and only then calls the route, at
  [timeline.html:1384](timeline.html#L1384).
- The call is a loop of small pages of conversations rather than one request, so the bar fills with
  real counts instead of spinning: [timeline.html:1303-1328](timeline.html#L1303-L1328), five
  conversations per call by default ([routes/detect.rs:43](backend/timeline-api/src/routes/detect.rs#L43)).

## Two ways "always" is still accurate, which you may also have meant

Both are things I read in [routes/detect.rs:79-132](backend/timeline-api/src/routes/detect.rs#L79-L132),
not behaviour I ran:

1. **The route never checks whether the work is already done.** It computes flags for every message
   it is given and writes over the stored automatic values. Tick the box on a second load of the
   same export and the whole scan happens again.
2. **It covers the whole account, not the file just uploaded.** It lists every conversation the
   signed-in name has and pages through all of them. So after a second upload, ticking the box
   rescans the first one too.

Neither is harmful as far as I can tell — the scan is repeatable and it never touches anything you
confirmed yourself — but "only what you just uploaded, only if not already scanned" is not what the
code does, and if that is what you expected, this is the gap.

## How the two differ

| | The box on the load screen | The "Classify with AI" button |
|---|---|---|
| Where the work happens | In the backend, in Rust | In the page, by calling `api.anthropic.com` from your browser |
| What decides | Fixed word lists and a sentiment score; a dictionary check so IRS and DARPA are not read as shouting | Sonnet reads each message with the Claude reply before it and judges |
| What it looks for | ALL-CAPS emphasis, criticism, anger | Criticism and anger only — never the caps flag |
| Cost | Nothing per message | Paid model calls; the earlier estimate for the ~2,200-message sample was about $1 to $3 |
| Where it stores the result | Server-side, through the write-automatic-flags port | Only in the open page, and in the file you download |
| Works where | Anywhere the backend is reachable | Only while the page is running as a live Claude artifact — see [timeline-project-decisions.md:388](timeline-project-decisions.md#L388) |
| Code | [routes/detect.rs](backend/timeline-api/src/routes/detect.rs) | [timeline.html:1809](timeline.html#L1809), batches of 30 at [timeline.html:1690](timeline.html#L1690) |

So: the box is the free scan, the button is the paid judgment, and the button is meant to be the
better of the two on sarcasm, negation and mixed tone —
[timeline-project-decisions.md:278](timeline-project-decisions.md#L278) records why it was built at
all, which was that you had corrected a great many messages by hand and got results the word lists
did not match.

## You are right that this will confuse people, and here is exactly where

Four specific places, all read in the current files:

1. **They write to the same one place and are told apart only by a three-letter label.** Both end up
   in `_claude_timeline_auto`. The review table prints "auto" or "AI" in a small column,
   [timeline.html:2479](timeline.html#L2479), with the explanation only in a hover title at
   [timeline.html:2462](timeline.html#L2462). Nothing on screen says these are two different
   features with two different prices.
2. **The two triggers sit in different places and neither mentions the other.** The box is on the
   load screen; the button is in the Review tab at [timeline.html:833-844](timeline.html#L833-L844).
   A person who ticks the box has no way to learn the button exists, and a person who presses the
   button is not told there is a free scan that would have cost nothing.
3. **A model-produced tag cannot survive being read back from the server.** The export route stamps
   every automatic flag `"source": "heuristic"` outright, [routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99),
   and there is no route that accepts model-produced automatic flags at all. So work the button did
   is either shown as if the word lists had produced it, or lost. This one I would call a defect
   rather than a naming problem.
4. **The button probably cannot work in the way the page is now loaded.** It reaches
   `api.anthropic.com` straight from the page and there is no key or go-between for it —
   [timeline.html:1685-1689](timeline.html#L1685-L1689) and
   [docs/plans/2026-09-09-rust-aws-backend-migration.md:8](docs/plans/2026-09-09-rust-aws-backend-migration.md#L8)
   both say so, and the second records a real rejection by the browser as the evidence. I have not
   pressed the button in the current setup, so treat this as reading, not measurement. If it is
   dead, a visible button that always fails is worse than no button.

## What has deliberately not been decided

How these two are named and explained to users is [tabled at your direction (line 489)](docs/plans/2026-09-28-frontend-quality-of-life.md#L489):
you said you would design that, and Phase 4 was written to change only when the free scan runs and
whether you can watch it, inventing no labels. So the confusion you are pointing at is a known,
open item, not an oversight — except for item 3 above, which is a wiring fault and is not covered by
that tabled design.

If it would help, I can write up the two-name question as its own plan, and separately propose a fix
for the stamped-`"heuristic"` fault, which is small and does not depend on the naming.
```

---

## C-q3-r1

**Variant on disk (not what reached the instance):** C  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work stopped, and what has actually been run

I answered by reading only — I ran no tests and wrote no code. So everywhere below, "a test
exists for this" means I read the test; it does not mean I watched it pass. Where I say something
was run, I am reporting what a document in the repository says was run, and I name the document.

## The last work was not on the tool

The four newest commits, all dated 2026-09-30, are about a side experiment on how a written
instruction changes the words I choose — not about the timeline tool. In order:

- [docs/plans/2026-09-30-vocabulary-instruction-experiment.md](docs/plans/2026-09-30-vocabulary-instruction-experiment.md)
  and its write-up [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md).
  The write-up is candid: it wanted ten runs per condition, got two, lost twenty runs entirely when
  the session that launched them ended, and says outright that its own way of counting words could
  not tell *hash-keyed* from *whose*, so the headline table does not mean what it looks like.
- A second, larger design at
  [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).
  **This one has not been carried out.** It names three files it will produce; none of the three is
  in [docs/analysis/](docs/analysis/), which holds only the first attempt's write-up. It also links
  a results file that does not exist yet, so that link is dead as the document stands.

### Uncommitted right now

- [CLAUDE.md](CLAUDE.md) — one sentence reworded, and a whole new section added ("Only use words
  that are already in the project"). The second experiment design says it swaps this file per
  condition and that losing your uncommitted work in it is worse than losing the experiment. It is
  in a swapped state at this moment, so check it before anything overwrites it.
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  — still being edited.
- Nothing is pushed: the branch is 55 commits ahead of `origin/main`.

The last change to the tool itself was 2026-09-29: [POST /_dev/reset](backend/timeline-api/src/routes/dev_reset.rs),
the browser tests calling it before each case, and
[backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs), which
builds what a deployed build would use and asserts every `/_dev/*` path answers 404 while the real
paths answer "not allowed in". That last one closes the single scariest gap in the repository: a
route that erases everything sitting one wiring mistake from something deployable.

## The page work is finished; the cloud work is not

[docs/plans/2026-09-28-frontend-quality-of-life.md:10-17](docs/plans/2026-09-28-frontend-quality-of-life.md#L10)
marks all six items done. I looked for each one and found it in both the page and a browser test:

| Item | In the page | Test I read |
|---|---|---|
| Start-up scripts and buttons | [scripts/dev-up.sh](scripts/dev-up.sh), [.vscode/tasks.json](.vscode/tasks.json) | [scripts/test-dev-up.sh](scripts/test-dev-up.sh) |
| Cover the calendar and the counts views | — | [e2e/views.spec.js](e2e/views.spec.js), 12 cases |
| Delete the page's own flag-finding code and its two word lists | [timeline.html](timeline.html) is now 3,110 lines / 119KB, down from the 66,839 lines / 752KB the plan measured | the above |
| Flag-finding runs only when asked, showing how far it has got | checkbox at [timeline.html:756](timeline.html#L756), loop at [timeline.html:1303](timeline.html#L1303), server side at [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs) | [e2e/views.spec.js:318](e2e/views.spec.js#L318) and [:331](e2e/views.spec.js#L331) |
| A real bar for the upload | [timeline.html:1176](timeline.html#L1176) uses `XMLHttpRequest` for the one request carrying the bytes | [e2e/views.spec.js:367](e2e/views.spec.js#L367) |
| Back and forward, and coming back to where you were | [timeline.html:2324-2349](timeline.html#L2324) | [e2e/views.spec.js:397](e2e/views.spec.js#L397), [:424](e2e/views.spec.js#L424), [:435](e2e/views.spec.js#L435), [:460](e2e/views.spec.js#L460) |

## Verified by someone running the real thing

These claims come from [backend/README.md:91-150](backend/README.md#L91), which states what was
driven by hand, and from tests that are committed and that I read:

- The local server answering real requests: no token rejected, a garbage token rejected, a request
  for a place to upload answered, an empty list for a new person, and a mark written then read back
  — all held as committed tests in [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs)
  driving the real router rather than a stand-in.
- Token checking in [backend/timeline-auth/](backend/timeline-auth/), including every rejection path
  that matters, against a real key pair and real signing.
- The in-memory stores, including that what the tool decides and what you decide about a message
  can never touch each other — enforced in three places, listed at
  [backend/README.md:174-193](backend/README.md#L174).
- The cross-built binary for the cloud: built for real, checked with `file`, and fed genuine
  cloud-shaped events through the local stand-in for the cloud runtime. Three such requests came
  back correct. **This was a session at a keyboard, not a committed test** — the README says so.
- Uploading a real file through the real server in a real browser, and a mark you approve surviving
  a reload: [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js), three cases, one of them a file
  over the 2MB body limit.

## Not verified, and honestly labelled as such

- **Nothing has ever reached Amazon.** [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs)
  and [backend/timeline-storage/src/dynamo/](backend/timeline-storage/src/dynamo/) compile, and I
  can see only one test module in the three of them
  ([backend/timeline-storage/src/dynamo/message_flags_table.rs:266](backend/timeline-storage/src/dynamo/message_flags_table.rs#L266),
  a temporary one reaching inside the file). There is no test file for either in
  [backend/timeline-storage/tests/](backend/timeline-storage/tests/) — all four cover the in-memory
  ones. Tracked as the still-open tenth entry in
  [docs/plans/2026-09-09-rust-aws-backend-migration.md:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926).
- **[infra/template.yaml](infra/template.yaml) has never been deployed or even checked for
  validity** — it says so in its own header at
  [infra/template.yaml:14-17](infra/template.yaml#L14). It also leaves out two things on purpose.
- **No real user pool, ever.** Only a throwaway key pair standing in for one. Fetching a real pool's
  keys over the network is not built.
- **Everything after the storage-and-sign-in step is unbuilt**: paid emotion classification through
  Amazon, the $5 charge and what it unlocks, and the hardening pass. Those are sections three
  through five of [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md).
- **The "Classify with AI" button in the page still calls `api.anthropic.com` straight from the
  browser** ([timeline.html:1688](timeline.html#L1688), [:1745](timeline.html#L1745),
  [:1756](timeline.html#L1756)). That is the broken-outside-its-original-home path the whole move to
  a server exists to fix, and it is untouched. It is also why your question about the new
  flag-finding step confusing people against this button is a live one: both are still in the page
  at once.

## Problems recorded as open, which I confirmed are still open

1. **The order conversations come back in is not fixed.** Recorded at
   [docs/plans/2026-09-28-frontend-quality-of-life.md:471](docs/plans/2026-09-28-frontend-quality-of-life.md#L471)
   with three runs of the same page giving three different orders. Still true:
   [backend/timeline-storage/src/memory/conversations.rs:14](backend/timeline-storage/src/memory/conversations.rs#L14)
   holds them in a `HashMap`, and `list_for_user` at
   [:36-48](backend/timeline-storage/src/memory/conversations.rs#L36) collects without ordering
   them. Only [backend/timeline-api/src/routes/detect.rs:82](backend/timeline-api/src/routes/detect.rs#L82)
   puts them in order, and it does that for its own paging. The two the page actually reads for
   display do not: [backend/timeline-api/src/routes/conversations.rs:16](backend/timeline-api/src/routes/conversations.rs#L16)
   and [backend/timeline-api/src/routes/export.rs:51](backend/timeline-api/src/routes/export.rs#L51)
   — the latter orders the upload identifiers on the next line but never the conversations. So the
   list and its colours still reshuffle between sessions.
2. **The page sometimes cannot reach the server and nobody knows why.** Recorded at
   [docs/plans/2026-09-28-frontend-quality-of-life.md:479](docs/plans/2026-09-28-frontend-quality-of-life.md#L479)
   as worked around by waiting for the server first, never explained. Nothing since claims a cause.
3. **How the two kinds of emotion-finding are named for people using the tool** is yours to design
   and is still waiting — [docs/plans/2026-09-28-frontend-quality-of-life.md:487](docs/plans/2026-09-28-frontend-quality-of-life.md#L487).
4. **The counted-words-dropped number in [timeline-project-decisions.md](timeline-project-decisions.md)
   still does not match the checked-in sample**, marked "likely settled, needs confirming" at
   [docs/plans/2026-09-09-rust-aws-backend-migration.md:890](docs/plans/2026-09-09-rust-aws-backend-migration.md#L890)
   and never confirmed against the original 4,482-message file.
5. **Turning on real emotion classification needs you to sign off on a live, paid call** —
   [docs/plans/2026-09-09-rust-aws-backend-migration.md:875](docs/plans/2026-09-09-rust-aws-backend-migration.md#L875).
   That sign-off has not happened, and that section cannot start without it.

## Three things marked done that I could not find

I am flagging these because they are the kind of gap this project's own rules say to surface rather
than smooth over. Each is a claim in a document that I could not match to anything on disk.

1. **"One command runs every test" is marked settled and I cannot find the command.**
   [docs/plans/2026-09-28-frontend-quality-of-life.md:444-452](docs/plans/2026-09-28-frontend-quality-of-life.md#L444)
   says the answer was one command covering the Rust tests, the browser tests, and the start-up
   script test, pinning the Node version too. [scripts/](scripts/) holds only `dev-up.sh`,
   `dev-down.sh`, `port-control.sh` and `test-dev-up.sh`; [.vscode/tasks.json](.vscode/tasks.json)
   has no such entry; there is no `Makefile`, no `justfile`, and no script entry in either
   [backend/package.json](backend/package.json) or [e2e/package.json](e2e/package.json). And
   [e2e/README.md](e2e/README.md) still ends by saying the browser tests run as part of nothing and
   must be remembered by hand. I did not find the command; I am not certain none exists, but I
   looked in every place I would expect it.
2. **The check for quietly-discarded errors still does not exist.** The same entry lists it as in
   scope; [docs/plans/2026-09-28-frontend-quality-of-life.md:117](docs/plans/2026-09-28-frontend-quality-of-life.md#L117)
   notes [CLAUDE.md](CLAUDE.md) points at a file that is not in this repository, and it still is
   not. Nor is there a replacement for either language. I can see the shape of what it would catch:
   [timeline.html:1075](timeline.html#L1075) and [:1444](timeline.html#L1444) discard a failure with
   only a comment, no record anywhere a reader of the logs could find it.
3. **The file-size check still only looks at Rust under [backend/](backend/).**
   [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) with an
   empty exemption list at [:17](backend/timeline-core/tests/file_sizes.rs#L17). Noted as remaining
   at [docs/plans/2026-09-28-frontend-quality-of-life.md:456](docs/plans/2026-09-28-frontend-quality-of-life.md#L456).
   Now that the page is 3,110 lines, a rule over it would finally mean something — and would fail,
   since the whole page is one file.

Also: **there is still no automated running of anything on a push** — no `.github/` directory.
Every test in this repository runs only when a person remembers to run it.

## Documents that have drifted out of date

- [backend/README.md:216](backend/README.md#L216) — the "not built yet" list names four things that
  have since been built: the step that turns an uploaded file into conversations and marks,
  `GET /export`, and (twice, in effect) the claim that "[timeline.html](timeline.html) itself is
  still 100% unmodified... nothing in the browser calls this backend yet." The page now calls
  `/uploads`, `/export`, `/conversations`, `/detect` and `_dev/login`
  ([timeline.html:1351](timeline.html#L1351), [:1270](timeline.html#L1270),
  [:1636](timeline.html#L1636), [:1309](timeline.html#L1309), [:1065](timeline.html#L1065)).
  Anyone reading that section today would be badly misled about where the project stands.
- [backend/README.md:66](backend/README.md#L66) says 163 tests. I counted 179 test markers across
  the four crates. Counting markers is not the same as counting what the runner reports, so treat
  this as "the two numbers disagree", not as a measurement — but the README's number is stale.
- The eighth concern in the page-improvements list is still tagged open at
  [docs/plans/2026-09-28-frontend-quality-of-life.md:402](docs/plans/2026-09-28-frontend-quality-of-life.md#L402)
  — silent return-to-where-you-were, or announced — while the table at the top of the same document
  says you chose announced and the code and a test both do that. Just a stale tag.

## The shortest honest summary

The tool works end to end on one machine, in a browser, with nothing saved once the server stops,
and a browser test suite drives that whole path. Everything that would make it a product people
could pay for — storage that survives, real sign-in, paid emotion classification, the charge, a
deployment — is either written and never once run against Amazon, or not written at all. The last
two days of work were not on the tool but on a side experiment about my own writing, and that
experiment's second round is designed but not run, with [CLAUDE.md](CLAUDE.md) left in a modified
state on disk.
```

---

## C-q3-r2

**Variant on disk (not what reached the instance):** C  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is verified versus what is not

Read-only pass. I did not run the tests and did not change any code. Everything below is either
something I read in a file (marked as read) or something a document claims (marked as claimed).

## Where it left off

The last four commits are documents only. The most recent, `966655d`, adds
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
— a second version of a written experiment about which instruction reduces words the reader has to
stop at. Before that, `57c8d3b`, `54c9e19` and `af47dae` wrote up the first attempt at that
experiment in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md).

The last commit that touched code is `d81b89e`, which empties the backend before each of the browser
tests in [e2e/views.spec.js](e2e/views.spec.js). The one before it, `d725cc3`, added
[backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs)
(`POST /_dev/reset`) and
[backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs), which
asserts that every `/_dev/*` path is absent from the build that could be deployed.

So: the six-phase pass in
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
is finished — all six rows of its own status table read **Done** (commit `721d940`) — and work then
moved off the product entirely, onto the writing experiment. Nothing is half-applied in the code as
far as I can see; the stopping point is clean.

**Two files are uncommitted in the working tree:**

1. [CLAUDE.md](CLAUDE.md) — a new section, *"Only use words that are already in the project"*, has
   been added by hand, along with the exemption for ordinary English words. This is your own
   uncommitted work; the experiment plan warns that losing this file is worse than losing the
   experiment.
2. [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
   — its three questions have been replaced with three of your own, and a constant line
   (*"Answer by reading only"*) is now appended to each.

By this project's own order of work, that plan is sitting at the point where it needs your approval
before anything runs.

## What the code now is

- [timeline.html](timeline.html) is **3,110 lines / 119,073 bytes** (read directly). The plan
  measured it at 66,839 lines / 752,370 bytes before the deletion, so the removal of the embedded
  English word list and the sentiment word list has landed.
- The backend has routes for uploads, export, detection
  ([backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)), the
  local-only login and byte store, and the local-only reset. Detection no longer runs at upload
  time; it runs when asked.
- [infra/template.yaml](infra/template.yaml) declares one bucket, three tables, a user pool and
  client, one function, and one HTTP interface (read).
- **No Amazon Bedrock code and no Stripe code exist anywhere** under `backend/*/src` or in
  [infra/template.yaml](infra/template.yaml) — I grepped; the single hit is a comment in
  [backend/timeline-core/src/ports/message_flags.rs:79](backend/timeline-core/src/ports/message_flags.rs#L79)
  saying Bedrock comes later. So the classification-by-Bedrock version, the $5 payment version, and
  the hardening version in
  [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
  are unstarted, exactly as that plan says.

## Verified

*Verified* here means a document states the code was run, and the committed tests it names exist as
files. I did not execute anything, so for the test suites I can vouch for the files, not for their
current pass/fail state.

- **The local server over real HTTP.** [backend/README.md](backend/README.md) records that
  unauthenticated and garbage-token requests were rejected, `POST /uploads` answered, an empty
  conversation list came back for a new user, and a flag write then read round-tripped — exercised
  by hand with `curl` and captured in
  [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs) against the real router.
- **Token checking**, including the rejection paths, against a real self-signed keypair and real
  signing — not a stub (same README section).
- **The in-memory stores**, through their real public methods, including that automatic flags and
  your own overrides cannot contaminate each other. That separation is enforced in three places, one
  of which is that the flag-writing handler has no automatic-writer parameter in scope at all.
- **A deployable binary really builds** — the README records `cargo lambda build --release --arm64`
  producing one.
- **`/_dev/*` is absent from that deployable build**, asserted by
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs), which
  also asserts the real routes are present and merely unauthorized, so it cannot pass on an empty
  router. This one matters most: `POST /_dev/login` mints a token with no password and
  `POST /_dev/reset` erases everything.
- **The page really works in a browser against a really running server.** Three tests in
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and fourteen blocks in
  [e2e/views.spec.js](e2e/views.spec.js) — one of which loops over the five analytics views — drive
  real Chrome: the calendar, a day click, opening a conversation, review search and paging, a
  backend-produced flag rendering as flagged, the annotated download, upload with no detection
  producing no flags, detection reporting progress, byte progress on the upload, the location
  written to the address and Back returning, and a reload restoring the session.
- **The page's deletion was output-neutral**, byte for byte, per that plan's own status row.

## Not verified

- **The real S3 and DynamoDB code has never run against anything real.** Stated as an open item in
  the migration plan's critique entry 10, and repeated in
  [backend/README.md](backend/README.md)'s coverage section: the untested part is concentrated in
  exactly those `send()` calls. There is no free, container-free stand-in for S3 yet; the downloadable
  DynamoDB substitute looks workable but is not wired up. **This is the next piece of real work** —
  the quality-of-life plan's own opening says the six phases were to finish *before* returning to it.
- **Nothing has been deployed, and no real user pool exists.** The README calls this out as a
  tracked gap, not a hidden one. The local login is a stand-in, not a login.
- **There is no continuous integration of any kind** — no `.github` directory (I checked). Every
  suite runs only when someone remembers.
- **The one command that runs every suite does not exist.** The quality-of-life plan's critique
  entry 12 is tagged `[RESOLVED]` and describes a single command covering the Rust workspace, the
  browser tests and the launcher test, plus a check for exceptions that get swallowed silently in
  both Rust and JavaScript. I grepped [scripts/](scripts/), [.vscode/tasks.json](.vscode/tasks.json),
  [backend/package.json](backend/package.json) and [e2e/package.json](e2e/package.json): no such
  command, and no swallow-check file anywhere in the repository. Only
  [scripts/test-dev-up.sh](scripts/test-dev-up.sh) (the launcher test) exists, and it says in its own
  header that it is not part of `cargo test`. [e2e/README.md](e2e/README.md) still says the browser
  tests run as part of nothing. **That entry is marked resolved and is not.**
- **The first known defect is still in the code.** I read
  [backend/timeline-storage/src/memory/conversations.rs:36-47](backend/timeline-storage/src/memory/conversations.rs#L36-L47):
  `list_for_user` filters and collects straight out of a `HashMap` with no sort, so conversation
  order still reshuffles between restarts, which the plan records as observed three times over.
- **The second known defect is unexplained.** The intermittent "Failed to fetch" from the page to a
  backend that is demonstrably answering was worked around by waiting for reachability, never
  root-caused.
- **The size ratchet cannot see the page.** I read `collect_rs_files` in
  [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs): it walks
  `.rs` files under `backend/` only. [timeline.html](timeline.html) is outside it, which is now worth
  fixing because at 3,110 lines a line-count rule would finally mean something.
- **The test count in the backend's own README is stale.** It says 163;
  [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
  records an audit finding 173 passing at 77.57% line coverage, and two test files
  ([backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs),
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)) have
  landed since. I did not run the suite, so I cannot give you the true number.
- **The dedup count mismatch is still an inference, not a fact.** Critique entry 8 in the migration
  plan explains a 4,482-versus-4,457 gap with arithmetic that matches one-for-one, but says plainly
  that the original file is gone and the cause was never confirmed. It also leaves you a decision:
  whether to correct the line in
  [timeline-project-decisions.md](timeline-project-decisions.md).
- **Capturing one real sample each from Bedrock and Stripe needs your sign-off** (critique entry 6,
  open) before the classification version starts.

## One gap in the experiment's own record

The writing experiment is the most recent work, and its record has a hole I should not paper over.

The first attempt reached **2 runs per condition against a target of 10** — 20 earlier runs were
lost entirely when the coordinating session ended. That is stated plainly in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md),
along with the finding that the best-scoring instruction produced the worst writing, and that the
measure could not tell a term of art from the word *whose*.

The second plan's uncommitted text quotes numbers that only a real attempt could have produced — ten
instances running at once against one machine and one port, and a run lasting 1,156 seconds — and
cites them to `docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md`. **That file does not
exist.** [docs/analysis/](docs/analysis/) holds exactly one file, the first attempt's. So at least
one attempt under the second plan has been made and its record is not on disk. None of the three
files the second plan promises exist, so the second version has not been run and recorded.
```

---

## C-q3-r3

**Variant on disk (not what reached the instance):** C  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work stopped, and what is actually backed by evidence

Everything below comes from reading files and git history. I did not run anything, so no
statement here means "I watched it work." Where the only evidence is a line of source, I say so.

## Where it stopped

Two things ended, one after the other.

**1. The six-part frontend clean-up is finished and committed.** The status table at the top of
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
marks all six done, and the commits match: launcher scripts plus buttons in the editor, browser
tests for the calendar and the five analytics views, deletion of the page's own detection code and
its two embedded word lists, detection moved out of upload and behind a checkbox with a progress
bar, an upload progress bar, and back/forward through `location.hash` with the session coming back
on reload.

**2. Attention then left the product entirely.** The four most recent commits are about a side
experiment — which written instruction makes me use fewer words the reader has not already seen.
That is [docs/plans/2026-09-30-vocabulary-instruction-experiment.md](docs/plans/2026-09-30-vocabulary-instruction-experiment.md),
what was observed in [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md),
and a second design in [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).
That second design is the live front. **Two files are uncommitted right now**: `CLAUDE.md`, which
has gained a section telling me to use only words already in the project (with a carve-out for
ordinary English), and that same second design.

So: the product work is at a natural resting point; the thing in flight is about how I write, not
about the timeline.

## What is backed by evidence

- **The last browser run passed.** [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json)
  holds `"status": "passed"` with an empty failure list, written 2026-09-30 04:14. That is a real
  record, not a claim in prose. It does not say which files ran, so it is weaker than "the whole
  suite passes."
- **Detection-only-when-asked is tested from outside the program, at two levels.**
  [backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs) asserts that an
  upload carries no automatic flags until detection is asked for, that the flags are the real
  computed ones rather than a stand-in, that an assistant turn never gets one, and that the paging
  covers every conversation exactly once. In the browser,
  [e2e/views.spec.js](e2e/views.spec.js) has a test that uploading without asking produces no flags
  at all, and one that progress is reported while the pass runs.
- **The developer-only routes are now asserted absent from the deployable build**, rather than
  believed absent from reading: [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs).
  This mattered more once a route that empties everything arrived.
- **The deletion really happened and the false promise is gone.** The page is now 3,110 lines /
  119,073 bytes, close to the ~112KB the plan predicted. The sentence telling the user their export
  "is read locally in this browser tab and is never uploaded anywhere" no longer appears anywhere in
  [timeline.html](timeline.html); I searched for it.
- **179 test functions exist in the backend** (counted across every `.rs` outside the build
  directory). I did not run them.

## What is not backed by evidence, in the order I would fix it

**1. The claim that "run all tests" now means one command is marked resolved, and I cannot find
it on disk.** [docs/plans/2026-09-28-frontend-quality-of-life.md:444](docs/plans/2026-09-28-frontend-quality-of-life.md#L444)
says one command now runs the Rust workspace, the browser suite and the launcher test, pins the
version of node the browser suite needs, and adds a check for errors that get quietly discarded.
What I actually find: [scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh`
and `test-dev-up.sh`; [.vscode/tasks.json](.vscode/tasks.json) has no test entry beyond the launcher
one; there is no top-level package, `Makefile` or equivalent; and no file anywhere checks for
discarded errors. The one piece that does exist is the node floor in
[e2e/package.json](e2e/package.json). **That entry overstates what landed** — this is the biggest
gap between the written record and the repository, and I would trust the repository.

**2. Both defects recorded during the deletion are still open**, as
[docs/plans/2026-09-28-frontend-quality-of-life.md:466](docs/plans/2026-09-28-frontend-quality-of-life.md#L466)
says they would be, and no commit since touches either:
- Conversation order still varies between restarts.
  [backend/timeline-storage/src/memory/conversations.rs:14](backend/timeline-storage/src/memory/conversations.rs#L14)
  keeps the summaries in a `HashMap`, and `list_for_user` at
  [line 36](backend/timeline-storage/src/memory/conversations.rs#L36) filters and collects with no
  sort. Read in source.
- The intermittent "Failed to fetch" from page to backend was worked around, never explained. No
  commit mentions it.

**3. Starting state for the browser tests is only half fixed.** The commit that added a clean slate
before each test changed one file. [e2e/views.spec.js:90](e2e/views.spec.js#L90) reuses a server
that is already listening and never stops one it did not start, and empties it before each test
through `POST /_dev/reset`. [e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53) was left as it
was: it starts a server unconditionally, never resets anything, and kills what it started at
[line 76](e2e/upload-flow.spec.js#L76). If a server is already up — which the launcher work makes
the normal case — the second one loses the port and those three tests quietly run against the first
one and whatever it already holds. That is exactly the trap the comment at
[e2e/views.spec.js:91-94](e2e/views.spec.js#L91-L94) describes. Read in source; I did not reproduce
it.

**4. The line-count ceiling cannot see the page.** [backend/timeline-core/tests/file_sizes.rs:36](backend/timeline-core/tests/file_sizes.rs#L36)
only picks up files ending `.rs` under `backend/`. The ceiling is 1,000 lines
([line 12](backend/timeline-core/tests/file_sizes.rs#L12)) and [timeline.html](timeline.html) is
3,110. Now that the file is small enough for a line count to mean something, this is worth doing.

**5. The coverage number in the plan is stale.**
[docs/plans/2026-09-28-frontend-quality-of-life.md:80](docs/plans/2026-09-28-frontend-quality-of-life.md#L80)
reports 173 tests and 77.57% of lines reached, measured before the deletion and before detection
moved. Neither has been measured since. [backend/README.md:66](backend/README.md#L66) still says
163. I counted 179.

**6. Nothing runs the tests automatically.** There is no `.github` directory of any kind, so every
suite runs only when someone remembers.

**7. The code that talks to the real object store and the real table has still never run against
anything.** Open, and honestly labelled as such, at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926).
This is the next real piece of server work, and the finished clean-up was explicitly sequenced
before it.

**8. Two questions are sitting with the user, not with me.** How the two kinds of emotion detection
are named and explained to users, which the user chose to design themselves
([docs/plans/2026-09-28-frontend-quality-of-life.md:487](docs/plans/2026-09-28-frontend-quality-of-life.md#L487));
and whether the parse step in the browser can ever show real progress, which cannot be settled
until someone looks at whether it still visibly freezes now the file is small
([docs/plans/2026-09-28-frontend-quality-of-life.md:364](docs/plans/2026-09-28-frontend-quality-of-life.md#L364)).

**9. The second experiment design has produced none of its three promised files**, and it cites one
of them for things it says already happened — ten instances running the test suite at once against
one machine, and a run lasting 1,156 seconds. Those observations currently have no recorded home
anywhere on disk. If that attempt is not going to be written up, the design should stop pointing at
a file that does not exist.

## Shortest honest summary

The product is at a clean stopping point with real tests behind the last round of changes, and the
work in flight is a writing experiment rather than the timeline itself. The two things I would not
take on trust are the entry claiming one command now runs every suite, which I cannot find in the
repository, and the starting state of the older of the two browser test files, which was never given
the clean slate the newer one got.
```

---

## E-q1-r1

**Variant on disk (not what reached the instance):** E  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and two of the three standard pieces are already in the repo — the third is the one that would
stop a future test file from forgetting the reset. Read-only answer; I ran nothing.

## What is already here

- The reset-through-a-test-only-route approach. [backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24)
  empties every in-memory store, reached over `POST /_dev/reset`; the emptying itself is the
  `Resettable` trait at [backend/timeline-storage/src/memory/resettable.rs:24-27](backend/timeline-storage/src/memory/resettable.rs#L24-L27),
  implemented only by the in-memory fakes. For a backend that runs as a separate process, this is
  the standard shape — the same idea as a test-only "flush" route. There is nothing more standard
  available, because no test framework can reach inside another process's memory.
- Running that reset before each test. [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130)
  calls it from `test.beforeEach`, with the reasoning written down at
  [e2e/views.spec.js:44-49](e2e/views.spec.js#L44-L49) and again in the route's own module comment.
  `test.beforeEach` is Playwright's normal hook for exactly this.
- Two places where the question does not arise at all, worth knowing so no effort gets spent there:
  - The browser half is already neutral. Playwright hands each test its own fresh browser context,
    so cookies, `localStorage` and `sessionStorage` start empty every time. That is why
    [e2e/views.spec.js:435](e2e/views.spec.js#L435) ("reloading restores the session") has to
    create the session inside the test — there is no leftover to clear. The opposite need, seeding
    state deliberately, is what `storageState` is for.
  - The Rust tests build a fresh app per test rather than resetting a shared one —
    [backend/timeline-api/tests/dev_routes.rs:34](backend/timeline-api/tests/dev_routes.rs#L34)
    constructs new stores and a new router each call. Isolation by construction, which is the
    better option whenever it is affordable; it is not affordable for the browser tests because
    each server start costs a `cargo run`.

## The standard mechanism not yet used, and why it matters here

Playwright's own way to attach setup to every test *across files* is a named setup value declared
once with `test.extend` and marked `auto: true`, exported from a shared file that each spec file
imports its `test` from. I confirmed both knobs exist in the installed version (1.63.0):
`auto?: boolean` at [e2e/node_modules/playwright/types/test.d.ts:6870](e2e/node_modules/playwright/types/test.d.ts#L6870),
and `webServer?:` at [e2e/node_modules/playwright/types/test.d.ts:1049](e2e/node_modules/playwright/types/test.d.ts#L1049)
with `reuseExistingServer` and `gracefulShutdown` alongside it.

Two concrete uses, in the order I would do them:

1. Move the reset into an automatic per-test setup value in a new shared file under
   [e2e/](e2e/), and have both spec files take `test` from there. A copied `beforeEach` is
   per-file and opt-in; an automatic setup value applies the moment a file imports that `test`, so
   a third spec file cannot silently skip it. Same effect for the reset that is already happening
   in [e2e/views.spec.js](e2e/views.spec.js), minus the chance of omission.
2. Replace the two hand-rolled server-start blocks with `webServer` in
   [e2e/playwright.config.js](e2e/playwright.config.js). Playwright starts the command, polls a
   url instead of sleeping, reuses an already-listening one when told to, and tears down the
   process tree itself. That is the framework doing what
   [e2e/views.spec.js:90-142](e2e/views.spec.js#L90-L142) and
   [e2e/upload-flow.spec.js:53-78](e2e/upload-flow.spec.js#L53-L78) currently do twice, by hand,
   differently. `workers: 1` at [e2e/playwright.config.js:14](e2e/playwright.config.js#L14) can
   stay for now; it is a separate decision from where the server gets started.

## Three problems I found while reading, which bear on this

- **[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets anything.** It has no
  `beforeEach` at all, so its three tests run against whatever the previous one left in the
  stores. The reset landed in one of the two files only. That is the direct answer to "tests can't
  assume a neutral starting state": right now half of them still do.
- **The two server lifecycles have diverged, and the older one can leak the port.**
  [e2e/views.spec.js](e2e/views.spec.js) spawns detached and kills the whole process group
  ([e2e/views.spec.js:88](e2e/views.spec.js#L88), [e2e/views.spec.js:132-141](e2e/views.spec.js#L132-L141));
  [e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77) sends `SIGTERM` to `cargo` only, which
  as I read it can leave the `timeline-api` child alive holding port 3000. Whichever file runs
  first starts the server, so the behavior you get depends on file order. I have not run this, so
  the orphaned-child part is inference from the spawn options, not something I observed.
- **Reuse-plus-reset can wipe a dev server you started by hand.** Port 3000 is hardcoded at
  [e2e/upload-flow.spec.js:17](e2e/upload-flow.spec.js#L17) and
  [e2e/views.spec.js:22](e2e/views.spec.js#L22), and
  [e2e/views.spec.js:90-100](e2e/views.spec.js#L90-L100) deliberately reuses anything already
  listening there. [scripts/dev-up.sh:9-12](scripts/dev-up.sh#L9-L12) exists precisely to avoid
  throwing away the in-memory session you are looking at, and the comment at
  [scripts/dev-up.sh:23-25](scripts/dev-up.sh#L23-L25) claims "the test suite … deliberately uses
  its own ports" — that is true of [scripts/test-dev-up.sh](scripts/test-dev-up.sh) (port 3999),
  but not of the browser tests. So running the browser tests while a hand-started backend is up on
  3000 empties it. If the reset becomes automatic for every test file, this gets easier to trigger,
  not harder. Reading the port from an environment variable with 3000 as the default, and having
  `webServer` start the suite's own instance on a different port, would settle it.

## Recommendation

One shared file under [e2e/](e2e/) holding an automatic per-test reset plus the server start via
`webServer`, both spec files importing `test` from it, and the port read from the environment. That
is entirely standard Playwright, removes the duplicated server code, closes the
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) gap, and does not require a new idea on the Rust
side — `POST /_dev/reset` is already the right primitive. It needs a plan in
[docs/plans/](docs/plans/) and your approval before any of it gets written.
```

---

## E-q1-r2

**Variant on disk (not what reached the instance):** E  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and most of it is already in the repo. There are two standard mechanisms, one per side, and
each is half-applied.

## 1. The browser tests: reset before every test, from one shared place

The reset itself already exists and is already wired:

- [backend/timeline-storage/src/memory/resettable.rs:24-27](backend/timeline-storage/src/memory/resettable.rs#L24-L27) —
  a trait with one method, `reset(&self)`, implemented only by the in-memory stores.
- [backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24) —
  `POST /_dev/reset` empties every store handed to it at
  [backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92).
- [e2e/views.spec.js:50](e2e/views.spec.js#L50) and
  [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) — `resetBackend()` called from
  `test.beforeEach`, so each test in that file starts empty.

The gap: [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets. It has a
`test.beforeAll` that starts the server ([line 53](e2e/upload-flow.spec.js#L53)) and nothing
per-test. Its three tests run against whatever the ones before them left behind.

The standard way to stop this being a per-file decision is Playwright's own shared-setup
mechanism: one module — say `e2e/backend.js` — that calls `test.extend` to wrap the imported
`test`, adding an entry marked to run for every test whether or not the test asks for it
(Playwright spells that `{ auto: true }`). Both spec files then `require` `test` from that module
instead of from `@playwright/test`, and a spec file added later cannot forget the reset, because
importing the test function is what enrols it. That is strictly better than copying the
`beforeEach` into the second file, which is the other option and which I'd expect to drift again.

The same module is the right home for starting and stopping the server, because that code is
currently duplicated: [e2e/upload-flow.spec.js:53-78](e2e/upload-flow.spec.js#L53-L78) and
[e2e/views.spec.js:90-140](e2e/views.spec.js#L90-L140) each carry their own `waitForPort`, their
own `spawn`, and their own tear-down, plus a `startedServerHere` flag in the second file whose only
job is to work out whether the first file already started one. Playwright's `globalSetup` /
`globalTeardown` settings in [e2e/playwright.config.js](e2e/playwright.config.js) exist for exactly
this: start it once for the whole run, stop it once at the end, and let every test assume it is
there. `workers: 1` ([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)) is what makes a
single shared server safe to reset between tests, so that line has to stay — or the reset has to
become per-worker with a port per worker.

## 2. The Rust tests: a fresh instance per test, which is already the pattern

Nothing to add here. [backend/timeline-api/tests/app.rs:38](backend/timeline-api/tests/app.rs#L38)
builds a whole new `AppState` with new in-memory stores for each test, and each test calls
`build_router(test_state())`. No process is shared, so there is no earlier state to inherit and
reset would be pointless —
[backend/timeline-api/tests/dev_routes.rs:58-60](backend/timeline-api/tests/dev_routes.rs#L58-L60)
says so explicitly, leaving the reset list empty on purpose. "Build the thing under test fresh in
each test" is the standard answer and it is the one in use.

## 3. Two things reset alone does not fix

Both are about the server process rather than its contents, and both bite exactly the way your
question describes.

**Killing the wrong process.**
[e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77) does
`serverProcess.kill('SIGTERM')` on `cargo`, not on its process group.
[e2e/views.spec.js:112-114](e2e/views.spec.js#L112-L114) spawns detached and signals the negative
pid instead, and its comment says why: signalling cargo alone can leave the server it launched
still holding the port. A server left running that way is inherited by the next run — where
[e2e/views.spec.js:95-100](e2e/views.spec.js#L95-L100) will reuse it deliberately, and
[e2e/upload-flow.spec.js:61](e2e/upload-flow.spec.js#L61) will spawn a second one that loses the
bind and then tests against the old server's data without saying so. I have not run the tests, so
I have not seen this happen; I am reporting that the two files handle the same tear-down
differently and that one of them documents the failure the other has.

**Port 3000 is someone's live session.** Both spec files hardcode
`http://127.0.0.1:3000` ([e2e/upload-flow.spec.js:17](e2e/upload-flow.spec.js#L17),
[e2e/views.spec.js:22](e2e/views.spec.js#L22)). That is the default port of
[scripts/dev-up.sh](scripts/dev-up.sh), whose whole restart-only-if-stale behaviour exists to avoid
throwing away the uploads and confirmed flags you are looking at
([scripts/dev-up.sh:9-13](scripts/dev-up.sh#L9-L13)). So running the browser tests against a
server that is already up now wipes it, per test.
[scripts/dev-up.sh:23-25](scripts/dev-up.sh#L23-L25) already claims the test suite "deliberately
uses its own ports" — true of [scripts/test-dev-up.sh:21-22](scripts/test-dev-up.sh#L21-L22),
which uses 3999 and 8999, and not true of the browser tests. Reading the port from the environment
with a non-3000 default in the browser tests would make the claim true and make the reset harmless.

## What I have not done

No code changed, and this needs a plan in [docs/plans/](docs/plans/) before any does. If you want
it, the scope I would write up is: the shared module with the automatic reset, server start/stop
moved into the run-wide setting, the process-group tear-down applied to both files, and a port for
the tests that is not the one you leave a session on.
```

---

## E-q1-r3

**Variant on disk (not what reached the instance):** E  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and there are two standard patterns rather than one. This repo already uses both — one in each layer — so the mechanism exists; what's missing is applying it uniformly. I read the code for this and did not run anything, per your instruction.

## The two patterns

**1. Build a fresh instance per test, so there is nothing to reset.** The Rust tests already do this: `test_state()` at [backend/timeline-api/tests/app.rs:38](backend/timeline-api/tests/app.rs#L38) makes new in-memory stores for every `#[tokio::test]`, and the requests go through `tower::ServiceExt::oneshot` with no listener and no shared process. This is the better shape whenever it's affordable, because a test cannot forget to do it.

**2. Share one instance and empty it in the framework's per-test setup hook.** That's the only option for the browser tests, since they drive one long-lived `cargo run -p timeline-api` on port 3000. Playwright's standard hook for this is `test.beforeEach`, and [e2e/views.spec.js:128](e2e/views.spec.js#L128) already calls `POST /_dev/reset` there. The route is [backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24); it walks the list registered at [backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92), and I checked each of the four in-memory stores (object store, conversation summaries, message flags, upload outcomes) does implement the `Resettable` trait at [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24).

## What reading turned up as not-yet-uniform

- **[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets.** It has only `beforeAll` at [line 53](e2e/upload-flow.spec.js#L53) and `afterAll` at [line 76](e2e/upload-flow.spec.js#L76) — no `beforeEach`. Its three tests inherit each other's uploads, and since [e2e/playwright.config.js:14](e2e/playwright.config.js#L14) runs one worker with both files on the same port, they can inherit the other file's data too. (Inferred from the config plus the code, not observed in a run.)
- **The two files disagree about who owns the server.** [e2e/views.spec.js:90](e2e/views.spec.js#L90) probes the port and reuses a server that is already listening, killing only one it started ([line 88](e2e/views.spec.js#L88)). [e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53) spawns unconditionally, not detached, and kills the `cargo` parent — so whichever runs second either loses the bind and silently tests against the other file's server, or leaves an orphan holding port 3000. That is a starting-state problem of a different kind: *which server*, not *which data*.
- **Both files duplicate** `waitForPort`, the spawn block, the API base, and the path to the sample export file ([e2e/views.spec.js:19](e2e/views.spec.js#L19), [e2e/upload-flow.spec.js:14](e2e/upload-flow.spec.js#L14)).

## What I'd employ

Two standard Playwright mechanisms, and I'd use them together:

- **Move the server's lifetime into the config's `webServer` option** ([e2e/playwright.config.js](e2e/playwright.config.js)). It runs a command once per suite, waits on a URL instead of sleeping, reuses an already-listening one via `reuseExistingServer`, and shuts it down at the end. That deletes both copies of the spawn/wait code and removes the who-kills-it ambiguity above.
- **Make the reset automatic rather than per-file.** One module in `e2e/` exports a `test` produced by `test.extend(..., { auto: true })` — a setup step Playwright runs before every test in every file that imports that `test`, without the file having to remember. Spec files then import `test` from there instead of from `@playwright/test`, and a new spec file gets the clean slate by construction. The cheaper version is just adding `test.beforeEach(resetBackend)` to [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and sharing `resetBackend` from a common module, but that keeps the "remember to add it" failure mode.

Two things worth not hand-rolling:

- **Browser-side state is already handled.** The page keeps `timeline_api_base` and `timeline_dev_sub` in localStorage ([timeline.html:1049](timeline.html#L1049), [timeline.html:1075](timeline.html#L1075)). Playwright gives each test its own browser context by default, and I grepped both spec files for `newContext`, `storageState` and `test.use` and found none, so nothing shares a context — no explicit localStorage clearing needed.
- **Neither pattern covers the real adapters.** `Resettable` is implemented only by the in-memory fakes, deliberately, so `POST /_dev/reset` cannot reach a real S3 bucket or DynamoDB table — and the `_dev` router is absent from the Lambda build. A clean-slate story for tests against LocalStack or real AWS does not exist yet; [e2e/README.md](e2e/README.md) points at the migration plan's C10 for that still-unbuilt work. I am not claiming that half is solved.
```

---

## E-q2-r1

**Variant on disk (not what reached the instance):** E  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Read-only answer. Nothing below was run; every claim is something I read in a
named file, and where I am guessing I say so.

## First, the premise no longer holds in the code as it stands

I went looking for the always-on behaviour and did not find it. Upload no longer
detects anything:

- [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98)
  says, in the doc comment on `process_upload`, "**This does not compute flags.**"
  Reading the body of that function, it fetches the raw object, parses, dedups,
  writes one summary per conversation, records an outcome, and stops.
- The scan now lives behind its own route, `POST /detect`, registered at
  [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27) and
  handled by [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs).
- In the page, it fires only from a checkbox that starts unticked
  ([timeline.html:756](timeline.html#L756)), read into `runDetection` at
  [timeline.html:1334](timeline.html#L1334) and guarding the call at
  [timeline.html:1384](timeline.html#L1384).

So the answer to "why does it always" is: it used to, it was written up as a
mistake, and the change landed. I have read the source only. I have not run the
tests and have not seen a request go through, so my status for this is
source-level, not observed at runtime.

## Why it used to always run

Not carelessness. It came out of a decision recorded at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:32-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32-L33):
a free scan costing about nothing per message, "always available", sitting under
one paid classification pass per $5. "Always available" and "already computed
for you at upload, asked for or not" are two different promises, and what got
built did the second while the plan only asked for the first. That is exactly the
reading in Phase 4 of
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196),
which also declines to defend it on speed, since nobody measured it.

## Yes, the two are genuinely different things

They differ on every axis that matters, and I can point at each one:

| | the scan behind the checkbox | the "Classify with AI" button |
|---|---|---|
| Where it runs | Rust, on the server, in [routes/detect.rs](backend/timeline-api/src/routes/detect.rs) | your browser, calling `api.anthropic.com` from [timeline.html:1745](timeline.html#L1745) |
| How it decides | word lists, a dictionary check for ALL-CAPS, sentiment scoring — `heuristic_flags` at [backend/timeline-api/src/processing.rs:84](backend/timeline-api/src/processing.rs#L84) | asks Claude (Sonnet) to judge each message, 30 per call, with the preceding Claude reply as context |
| What it can set | all three: caps, critical, angry | critical and angry only — [timeline.html:1848-1850](timeline.html#L1848-L1850) never touches `default_caps` |
| Cost | free | money, estimated up front in [timeline-project-decisions.md:278](timeline-project-decisions.md#L278) |
| Where you find it | the load screen, before anything is on screen | inside the "Review & flags" tab, [timeline.html:835-844](timeline.html#L835-L844) |
| Works when | whenever the backend is reachable | only while the page is live inside a Claude artifact; a downloaded copy of the file cannot reach the API at all |

And they land in the same place. Both write the *automatic* value, leaving
anything you confirmed by hand alone. The one distinguishing mark is a `source`
field, rendered in the Review table at
[timeline.html:2479](timeline.html#L2479) as the word `auto` for the word-list
scan and `AI` for the Claude pass, with a hover title at
[timeline.html:2462](timeline.html#L2462).

## So it will confuse users, and here is where, specifically

Four places, in the order a person would hit them.

1. **The two are never named as a pair anywhere.** The checkbox copy at
   [timeline.html:758-765](timeline.html#L758-L765) describes what it looks for
   and never says a second, better, paid judgment exists later. The
   "Classify with AI" copy says it is "more accurate than the
   keyword/sentiment heuristic below" — but "below" is the table, and the thing
   being compared against is a checkbox on a screen that is gone by then. A
   person who skipped the checkbox reads "more accurate than" a thing they never
   ran.

2. **`auto` versus `AI` is doing too much work.** Those are the only two words
   distinguishing a word-list guess from a paid Claude judgment, they sit in a
   four-character column, and "AI" is also the most natural word a person would
   apply to *both*, since both are the computer guessing rather than them
   deciding. Phase 4 of
   [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
   records that you tabled how to name the two and will design that separately,
   so I am not proposing words here.

3. **Running the scan silently throws away the paid results, and the
   interface says the opposite.** The checkbox copy promises "This never
   overwrites flags you've confirmed yourself" — true, those are stored apart.
   What it does not say is that the Claude judgments are *not* yours in that
   sense; they are automatic, so a later scan replaces them. Worse, the scan can
   only ever write `caps`/`critical`/`angry` with no record of where they came
   from: `FlagSet` at
   [backend/timeline-core/src/ports/message_flags.rs:33-37](backend/timeline-core/src/ports/message_flags.rs#L33-L37)
   has three booleans and no `source`, and the export route hard-codes
   `"source": "heuristic"` at
   [backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99).
   The backend has no way to hold, or to admit to, an AI-sourced automatic flag.

4. **Following from 3: saving and reloading can relabel paid work as free
   work.** Reading the model, unknown fields survive parsing (`extra`, flattened,
   at [backend/timeline-core/src/model.rs:164-165](backend/timeline-core/src/model.rs#L164-L165)),
   so a file you saved after classifying carries `source: "llm"` back in. Reload
   it without ticking the box and the page still shows `AI`, because nothing
   overwrites it. Tick the box and the export replaces that record with the
   word-list result labelled `heuristic`. Same file, same two clicks in a
   different order, two different answers and no warning either way. I read this
   off the three files above; I have not reproduced it, so treat it as read in
   source rather than seen happen.

## What I would change, smallest first

- Fix the sentence that compares one to "the heuristic below" — it points at
  nothing the reader can see.
- Say on the checkbox that a later, paid, more careful pass exists, and say on
  the button that it replaces the free scan's two judgments and leaves ALL-CAPS
  alone.
- Give `FlagSet` a field for where an automatic value came from, so the two
  passes stop being indistinguishable to the backend and item 4 stops being
  possible. That is a change to stored data and belongs in a plan, not in a
  reply.

One thing I should be plain about: [timeline.html:743](timeline.html#L743) still
tells the reader "nothing is uploaded anywhere", which the upload route makes
false. That is not your question, and Phase 3 of the frontend plan already
flags it, but it is the same load screen and I am not going to mention the copy
there without mentioning it.
```

---

## E-q2-r2

**Variant on disk (not what reached the instance):** E  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
## First, the part of the question I have to correct: it doesn't any more

The code on disk today does not detect at upload. Three places say so:

- The doc comment over the upload path says it outright — "**This does not compute flags.**
  Detection is a separate, user-triggered pass" —
  [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98), above
  [`process_upload` at :105](backend/timeline-api/src/processing.rs#L105), whose body writes one
  summary per conversation and records the outcome and nothing else.
- The pass now lives behind its own request,
  [backend/timeline-api/src/routes/detect.rs:1](backend/timeline-api/src/routes/detect.rs#L1),
  which shares the same three checks through
  [`heuristic_flags` at processing.rs:84](backend/timeline-api/src/processing.rs#L84).
- The page asks for it only if you tick a box that starts unticked —
  [timeline.html:756](timeline.html#L756), its explanation at
  [timeline.html:760](timeline.html#L760) ("This runs only if you tick it"), read at
  [timeline.html:1334](timeline.html#L1334) and acted on at
  [timeline.html:1384](timeline.html#L1384). The page also no longer checks anything itself:
  [timeline.html:999-1005](timeline.html#L999-L1005) reads whatever the server stored, and a
  message with nothing stored simply has nothing.

Two limits on that answer. I read the source and was asked not to run anything, so this is what
the code says, not a run I watched. And the row marked **Done** in
[docs/plans/2026-09-28-frontend-quality-of-life.md:15](docs/plans/2026-09-28-frontend-quality-of-life.md#L15)
is that plan's claim about an earlier run, not mine.

**Why it used to.** Not an oversight. The migration plan's confirmed list at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:32](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32)
promises the free word-list pass is "always available", and what got built read that as "already
computed at upload, whether or not anyone asked". Those are two different promises, and your
question is what got the second one taken back — it is quoted in the heading at
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196).

## Second: yes, it is a different thing from the "Classify with AI" button

| | the box on the load screen | the "Classify with AI" button |
|---|---|---|
| What decides | fixed word lists, a dictionary check for ALL-CAPS, and sentiment scoring, in Rust ([routes/detect.rs:1](backend/timeline-api/src/routes/detect.rs#L1)) | Claude (Sonnet) reads each message with the reply it answered, and judges ([timeline.html:1809](timeline.html#L1809)) |
| Where it runs | the server | the page, calling out to the model |
| Money | none | real tokens, roughly a dollar to a few dollars for a large export |
| Where it works | anywhere the server is reachable | only while the page is open live as a Claude artifact; a copy of the file opened from disk cannot reach the model at all ([timeline.html:1685-1689](timeline.html#L1685-L1689)) |
| Where the result is kept | stored on the server, so it survives a reload | in the page only, plus a part-way copy for the run in progress ([timeline.html:1601](timeline.html#L1601)); the comment right above says it is no longer read back on the next load |
| Judges ALL-CAPS | yes | no — only critical and angry |
| Small label on the row | `auto` | `AI` ([timeline.html:2479](timeline.html#L2479)) |

What they have in common is the part that makes them look like one thing: both fill the *same*
three boxes on the same rows, and both leave anything you ticked yourself alone. So the second one
run after the first one overwrites the first one's answers, and the row looks the same either way
apart from that two-or-four-letter label.

## Third: you are right that it will confuse people, and here is exactly where

1. **Two controls, two places, one result.** A tick box on the load screen before you ever see the
   timeline, and a button inside the Review tab ([timeline.html:833-844](timeline.html#L833-L844)).
   Nothing on either one says the other exists, or that the second overwrites the first.
2. **Nobody has named them.** The plan records that you tabled how the two get named and explained
   ([docs/plans/2026-09-28-frontend-quality-of-life.md:15](docs/plans/2026-09-28-frontend-quality-of-life.md#L15)),
   so what a person sees now is "scan for likely ALL-CAPS emphasis, criticism of Claude, and anger"
   in one place and "Classify with AI" in another, with no word shared between them. The row label
   is `auto` versus `AI`, which reads as *automatic* against *clever* rather than as two named
   things a person could pick between.
3. **One line of the wording is now wrong.** The button's box says the model is "more accurate than
   the keyword/sentiment heuristic **below**"
   ([timeline.html:838](timeline.html#L838)). Nothing below it does that any more — the control
   that does moved to the load screen. As written it sends the reader looking for a thing that
   isn't there.
4. **The two results do not last equally long, and look identical while they do.** The word-list
   answers are on the server. The model's answers are in the page, and the comment at
   [timeline.html:1592-1600](timeline.html#L1592-L1600) says they are not recovered on the next
   load. So after a reload, rows silently fall back to the word lists' answers — or to nothing —
   with no notice that the more expensive pass was dropped.
5. **The button is always there and always clickable.** It is wired unconditionally
   ([timeline.html:2633](timeline.html#L2633)) and nothing checks first whether the page can reach
   the model. Outside a live artifact, a person pays a confirmation step and a progress bar to get
   a batch-by-batch failure.

## What would fix it, if you want it fixed now rather than in the tabled pass

- Put both in one place, as one choice with two answers, and make each say what the other costs and
  what it overwrites. That is the tabled question, so it is your call, not mine.
- Correct the word "below" at [timeline.html:838](timeline.html#L838) regardless of the rest; it is
  wrong today and is a one-line edit.
- Hide or disable the button when the page cannot reach the model, instead of failing after the
  confirmation.
- Send the model's answers to the server the way ticked-by-hand ones already go
  ([timeline.html:1636](timeline.html#L1636)), so the two stop differing in how long they last. The
  comment at [timeline.html:1599-1600](timeline.html#L1599-L1600) says this gap is left open on
  purpose because that button is due to be replaced by the paid server-side pass; if that
  replacement is far off, this is the cheaper fix in the meantime.
```

---

## E-q2-r3

**Variant on disk (not what reached the instance):** E  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
# Why does the backend always detect?

## Short answer: in the code as it stands, it no longer does

Commit 6c7d9a9 ("Move detection out of upload into a user-triggered POST /detect") changed this.
What I read in the current files:

- Upload now only parses, removes duplicates, stores one summary per conversation, and records that
  it finished. The docstring on `process_upload` says so in as many words:
  [backend/timeline-api/src/processing.rs:100-108](backend/timeline-api/src/processing.rs#L100-L108)
  — "**This does not compute flags.** ... A freshly uploaded export therefore has no automatic flags
  until detection is requested, which is the intended behavior and not a missing write."
- The scan lives behind its own route,
  [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs), registered
  at [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27).
- The page calls it only if a box on the load screen is ticked. The box is at
  [timeline.html:756](timeline.html#L756) and is unchecked in the markup; the call is guarded at
  [timeline.html:1384](timeline.html#L1384) (`if(runDetection)`), and the loop that drives it is
  [`runDetectionPass`](timeline.html#L1303).
- With the box left alone, every message you sent arrives with no automatic flags at all —
  [timeline.html:997-1006](timeline.html#L997-L1006) reads them as absent and marks the source
  `'none'`.

This is code I read, not a run I watched: I did not start the server or the browser tests, so I can
say the source now works this way and cannot say I have seen it behave this way.

## Why it used to run on every upload

That was a decision, not an oversight, and it is written down in the migration plan at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:32-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32-L33):
a word-list-and-sentiment scan costing about nothing per run, described as *always available*, set
against one paid classification per $5 of usage. "Always available" then got built as "already
computed before anyone asked for it," and those are not the same promise. Your question is what
prompted the change; it is recorded as Phase 4 of
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196),
which also notes that the reason to change it was control and clarity, not speed — nobody has
measured how long the scan takes, which is also why it now shows progress while it runs.

## Yes, it is a different thing from the "Classify with AI" button

Every difference below is from reading the two code paths.

| | The box on the load screen | The button in "Review & flags" |
|---|---|---|
| Where the work happens | On the server, in Rust: [routes/detect.rs](backend/timeline-api/src/routes/detect.rs) calling [`heuristic_flags`](backend/timeline-api/src/processing.rs#L88) | In your browser tab, which posts straight to `api.anthropic.com` ([timeline.html:1852](timeline.html#L1852)) and only works while the page is open as a live Claude artifact ([timeline.html:1685-1689](timeline.html#L1685-L1689)) |
| What it reads | The text of each message you sent, nothing else | Each message plus up to 300 characters of Claude's reply just before it ([timeline.html:1691-1700](timeline.html#L1691-L1700)) |
| How it decides | A dictionary check for ALL-CAPS words that are not acronyms, plus word lists and sentiment scoring: [backend/timeline-core/src/flags/](backend/timeline-core/src/flags/) | Asks `claude-sonnet-4-6` for a true/false judgement per message ([`buildClassifyPrompt`](timeline.html#L1711)) |
| Which of the three flags it sets | All three — ALL-CAPS, critical, angry | Only critical and angry. It never touches ALL-CAPS ([timeline.html:1846-1849](timeline.html#L1846-L1849)) |
| What it costs | No tokens | Tokens, one call per 30 messages, with a confirmation dialog first ([timeline.html:1816-1821](timeline.html#L1816-L1821)) |
| Where the result is kept | Written on the server through `AutoFlagWriter`, and comes back inside the export marked `"source": "heuristic"` ([routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)) | Only in the page's memory, plus a part-way checkpoint that is deliberately **not** read back on the next load ([timeline.html:1592-1600](timeline.html#L1592-L1600)). It survives a reload only if you press "Download annotated conversations.json", which writes `source: 'llm'` ([timeline.html:1928](timeline.html#L1928)) |

How they interact: the button overwrites critical and angry wherever they came from the word lists,
and leaves anything you ticked yourself alone. So the two are not alternatives you pick between —
the second one, when it works, replaces part of the first one's output.

## And you are right that it will confuse users — that part is still open

Phase 4 says so itself, at
[docs/plans/2026-09-28-frontend-quality-of-life.md:230-238](docs/plans/2026-09-28-frontend-quality-of-life.md#L230-L238):
you tabled how the two get named and explained, so that change deliberately altered *when* the scan
runs and nothing about how either one is described. Three specific things I can see in the page that
would mislead someone:

1. **Neither has a name that relates it to the other.** The load-screen box is a sentence with no
   title — "After uploading, scan for likely ALL-CAPS emphasis, criticism of Claude, and anger"
   ([timeline.html:758](timeline.html#L758)) — and the other is a button reading "Classify with AI"
   ([timeline.html:844](timeline.html#L844)). Nothing on either tells you the button rewrites what
   the box produced, or that the box is the only source of the ALL-CAPS flag.
2. **Skipping the scan looks identical to running it and finding nothing.** With the box unticked
   every flag is false, and the Review table still labels each one "auto"
   ([timeline.html:2479](timeline.html#L2479)) — the same word it uses when the scan really ran. The
   hover text that would distinguish them is empty in that case, because it is only filled in for
   the `'heuristic'` and `'llm'` sources ([timeline.html:2462](timeline.html#L2462)). So a person who
   leaves the box alone sees a clean table and can reasonably read it as "I was not critical or
   angry once."
3. **One result is kept and the other is not.** Ticking the box changes stored server data; pressing
   the button changes only what is on screen until you download the annotated file. Nothing in the
   interface says this.

The cheapest fix for (2) is separable from the naming question you want to design yourself: show the
`'none'` case as something other than "auto" — a dash, or a line above the table saying no scan has
been run and offering to run one. That needs no decision about what the two passes are called.
```

---

## E-q3-r1

**Variant on disk (not what reached the instance):** E  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is checked versus not

Answered by reading only: git history, the three documents under [docs/](docs/), the Rust sources
under [backend/](backend/), the browser tests in [e2e/](e2e/), and [infra/template.yaml](infra/template.yaml).
Nothing was run. Every claim below is marked as either read-in-source or taken-from-a-document.

## The last work on the product itself

Three commits on 2026-09-29 at 14:40, all about the browser tests being able to start from an empty
server:

- `da1457a` let the in-memory stores be emptied,
- `d725cc3` added `POST /_dev/reset` plus a test that the `_dev` paths stay out of the build that
  would be deployed ([backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)),
- `d81b89e` called that reset before each browser test,
- `7cb86d9` marked critique item C11 in the quality-of-life document resolved.

That closed the sixth and last phase of
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md).
Its table at [line 12 onward](docs/plans/2026-09-28-frontend-quality-of-life.md#L12) shows all six
phases **Done**: the start-up scripts, the browser-test coverage for the calendar and the analytics
views, the deletion of the page's own flag-finding code and its two word lists, flag-finding moved
behind a user action with visible progress, an upload progress bar, and Back/Forward inside the page.

## What has happened since

Nothing on the product. From 2026-09-30 01:40 to 03:56 the commits are all about a side experiment
on writing instructions: [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md),
[docs/plans/2026-09-30-vocabulary-instruction-experiment.md](docs/plans/2026-09-30-vocabulary-instruction-experiment.md),
and [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).

Two files are changed and not committed:

1. [CLAUDE.md](CLAUDE.md) now carries a section called "One word, one meaning" at
   [line 77](CLAUDE.md#L77). That text is one of the seven variants the second experiment plan lists
   (its row **E**, at [line 38 of the plan](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L38)).
   So the instruction file is currently holding an experiment variant rather than a settled rule.
   The same plan says at [line 149](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L149)
   that this file carries uncommitted work and losing it is worse than losing the experiment.
2. The second experiment plan itself, rewritten to use three of the user's own questions and to drop
   two that made instances run the test suite and write throwaway programs.

**The open front of work is therefore the second experiment, planned but not run.** The plan asks for
ten runs of each of seven conditions, seventy in all
([line 93](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L93)), and names three files
it will produce ([line 126 onward](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L126)).
None of the three exist: [docs/analysis/](docs/analysis/) holds only the first attempt's results.

## Checked, with the evidence

- **The `_dev` paths cannot reach a deployed build.** A test builds exactly what the deployed branch
  builds and asserts each `_dev` path answers "not found", and that the real paths are present but
  refuse an unauthenticated caller ([backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)).
  This is the one thing whose being wrong would mean shipping a route that mints tokens for anybody.
- **The browser tests are real and broad.** [e2e/views.spec.js](e2e/views.spec.js) holds fifteen
  tests and [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) three, driving a real browser against
  a real server started by the suite: the calendar, a day click, opening a conversation, each of the
  five analytics views, review search and paging, a flag coming from the server, the annotated
  download, an upload with no flag-finding asked for, progress while flag-finding runs, byte progress
  during upload, the address bar changing with the view, and reload restoring the session.
- **Counted by test attribute lines, 168 Rust test functions across 25 test files** under
  [backend/](backend/) (read by counting; cases generated inside a test, such as the property-based
  dedup test, add more).
- **The last browser-test run reported success.** [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json)
  says `"passed"` with no failures. Two cautions: its timestamp is 2026-09-30 04:14, which is after
  the last product commit and inside the experiment window, so nothing records who ran it or against
  what change; and a pass marker is not the same as a run whose purpose was to check a change.
- **Deleting the page's own flag-finding changed nothing on screen** — the quality-of-life document
  records this as checked byte for byte
  ([line 15](docs/plans/2026-09-28-frontend-quality-of-life.md#L15)). Taken from the document; I did
  not re-do it.
- **The page is now readable**: [timeline.html](timeline.html) is 3,110 lines and 119,073 bytes,
  against the 66,839 lines and 752,370 bytes the document measured before the deletion
  ([line 126](docs/plans/2026-09-28-frontend-quality-of-life.md#L126)).

## Not checked, and some of it worse than the documents say

### 1. The AWS storage code is never called at all

[backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) (96 lines) and
[backend/timeline-storage/src/dynamo/](backend/timeline-storage/src/dynamo/) (630 lines) name no
caller anywhere else: searching every `.rs` file under `backend/*/src` and `backend/*/tests` for
`S3ObjectStore`, `DynamoConversations` and `DynamoMessageFlags` finds them only inside those files.
The migration document's C10 at
[line 926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926) says this code has never run
against anything real, which is true and understated: it is also never built into anything that runs.

### 2. A deployed build would keep data in memory and trust a signing key it made up

Read in [backend/timeline-api/src/main.rs](backend/timeline-api/src/main.rs): the branch taken when
the deployment runtime is present, at [line 117](backend/timeline-api/src/main.rs#L117), calls the
same `build_local_state` at [line 49](backend/timeline-api/src/main.rs#L49) as local running. That
function builds the four in-memory stores and, at
[line 81](backend/timeline-api/src/main.rs#L81), a token checker pointed at the throwaway key pair
generated in the process. Searching every source file for `TIMELINE_` finds no match, so the bucket
name, the three table names, and the user-pool and client identifiers that
[infra/template.yaml](infra/template.yaml) passes in are read by nothing. A deploy would therefore
lose every upload between calls and accept tokens signed by a key it invented.
This is read in source; no deploy has been attempted, and nothing in the documents claims one has.
The documents do not state this particular gap anywhere I found.

### 3. The whole deployed step of the migration, and everything after it

[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
lists at [line 188](docs/plans/2026-09-09-rust-aws-backend-migration.md#L188) what its second step
needs before being called done: tests against a free local stand-in for the storage services, a
rejected-token test against a real user pool, a 60-megabyte upload staying inside the function's
memory and time limits, and one run against real low-volume storage. None of that exists. The
Bedrock classification step at [line 530](docs/plans/2026-09-09-rust-aws-backend-migration.md#L530),
the payment step at [line 562](docs/plans/2026-09-09-rust-aws-backend-migration.md#L562), and the
hardening step at [line 591](docs/plans/2026-09-09-rust-aws-backend-migration.md#L591) are unstarted.

### 4. "One command runs every test" is recorded as resolved but is not on disk

C12 at [line 444](docs/plans/2026-09-28-frontend-quality-of-life.md#L444) is tagged `[RESOLVED]` and
its resolution says one command runs the Rust tests, the browser tests and the start-up-script test,
and pins the Node version. I find no such command: [scripts/](scripts/) holds only `dev-up.sh`,
`dev-down.sh`, `port-control.sh` and `test-dev-up.sh`; there is no `Makefile`, no `justfile`, no
package file at the top of the repository, and no `.github` directory. The Node version appears only
as an `engines` field in [e2e/package.json](e2e/package.json#L12), which `npm install` does not
enforce unless told to. The same C12 entry says a check for quietly discarded errors was also in
scope; `tests/test_no_unhandled_exceptions.py`, which [CLAUDE.md](CLAUDE.md) refers to, does not
exist, and there is no `tests` directory and no Python in this repository.

### 5. Both known defects are still there

Listed at [line 466](docs/plans/2026-09-28-frontend-quality-of-life.md#L466):

- Conversation order still varies between server restarts. `list_for_user` at
  [backend/timeline-storage/src/memory/conversations.rs:36-48](backend/timeline-storage/src/memory/conversations.rs#L36-L48)
  still walks the map and collects with no sorting, so the fix described in that entry has not been
  made.
- The page intermittently reporting that it cannot reach the server was worked around by waiting for
  the port, never explained.

### 6. Smaller open items

- The file-length check at [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs)
  reads only `.rs` files under [backend/](backend/), so [timeline.html](timeline.html) is outside it.
- The coverage figure quoted in the quality-of-life document (173 Rust tests, 77.57% of lines) was
  measured during phase 2; the four later phases added code and tests, so that number is stale and I
  did not re-measure it.
- Still tagged open: C6 ([line 875](docs/plans/2026-09-09-rust-aws-backend-migration.md#L875), needs
  the user's go-ahead to capture one real model call), C8
  ([line 890](docs/plans/2026-09-09-rust-aws-backend-migration.md#L890), a duplicate-message count in
  [timeline-project-decisions.md](timeline-project-decisions.md) that the measured file contradicts),
  C10 above, and in the quality-of-life document C1
  ([line 364](docs/plans/2026-09-28-frontend-quality-of-life.md#L364), the reading step in the page
  cannot show progress) and C8 ([line 402](docs/plans/2026-09-28-frontend-quality-of-life.md#L402),
  now answered: restoring a session announces itself).
- How the two kinds of emotion-finding are named for users is set aside at the user's direction
  ([line 487](docs/plans/2026-09-28-frontend-quality-of-life.md#L487)).

### 7. Two problems inside the experiment documents themselves

- [docs/analysis/2026-09-30-vocabulary-instruction-results.md:197](docs/analysis/2026-09-30-vocabulary-instruction-results.md#L197)
  still reads "No timing conclusion is available." The body above it reaches the opposite conclusion
  and says so explicitly. Commit `57c8d3b` fixed two other sentences the same discovery contradicted
  and missed this one.
- The second plan sends the reader to
  `docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md` at
  [line 61](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L61) for the record of the
  two retired questions. That file does not exist, so the first attempt at the second experiment —
  its crowding, its 1,156-second run, the programs it wrote — is described only in the plan that
  points elsewhere for it.
- The first experiment reached 2 runs per condition out of 10: 20 runs were launched and all 20 lost
  when the session that started them ended. The results document says so plainly on its first page
  and rests every number on those 2.

## In one paragraph

The browser-facing work is finished and is the best-checked part of the project: six phases done, a
real browser exercising fifteen views and flows against a real server, and the page cut from 752
kilobytes to 119. Everything about running this anywhere other than one machine is unchecked, and
further from working than the documents say: the storage code for the cloud services is written but
called by nothing, and the branch meant for deployment would keep data in memory and trust a signing
key it generates for itself. Two known defects are still in the code, the command that is supposed to
run every test does not exist despite being marked resolved, and the actual current activity is a
side experiment about writing instructions whose second round is planned but not run, with the
instruction file left holding one of the variants uncommitted.
```

---

## E-q3-r2

**Variant on disk (not what reached the instance):** E  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work left off, and what is verified

Everything below comes from reading files and git history. I ran no tests and no build, so
every claim of the form "the tests pass" is a claim *a document in the repository makes*, not
something I watched happen.

## The last thing done

The last change to any code was commit `d81b89e`, 2026-09-29: "Reset the backend before each
e2e test", which touched only [e2e/views.spec.js](e2e/views.spec.js). Every commit since then,
all dated 2026-09-30, is a document about a side experiment on writing style, not product code.

Two files are modified and not committed right now:

- [CLAUDE.md](CLAUDE.md) — adds a section titled "One word, one meaning" (each word carries one
  meaning throughout; ordinary English is exempt), and tightens one sentence about finishing an
  already-approved plan.
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  — swaps in a new set of questions and adds a line telling each run to answer by reading only.

## Three threads, in the order they were worked

### 1. The browser-page cleanup — finished, by the plan's own marks

[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
holds six numbered phases, and the table at the top marks all six **Done**: the dev-server start
scripts, browser-test coverage for the calendar and the five analytics views, deleting the page's
own flag-detection code and its two embedded word lists, moving detection behind a button the
user presses, an upload progress bar, and back/forward navigation.

One of those I could check directly. [timeline.html](timeline.html) is now 119,073 bytes across
3,110 lines. The plan records it at 752,370 bytes and 66,839 lines before the deletion, 85% of
which was an embedded English word list and a sentiment word list. So the deletion did happen at
roughly the stated size. The plan claims the page's rendered output was identical before and
after, byte for byte; I did not re-derive that.

### 2. The Rust backend move to Amazon services — the next thing, deliberately parked

[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
is the older, larger plan. The page-cleanup plan says in its opening lines that those six phases
were to be finished *before* returning to this plan's server work. That server work — making the
real Amazon S3 and DynamoDB storage code actually run — is therefore what is next, and it has not
started.

### 3. An experiment on writing instructions — designed, partly run, about to be rerun

This is what the last four commits are. The question being measured: which written instruction in
[CLAUDE.md](CLAUDE.md) most reduces words a reader has to stop at.

- [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md)
  reports the first attempt. Twenty runs were launched in the background and **all twenty were
  lost** — the coordinating session ended first and nothing was written. Six were then rerun in
  the foreground and finished, two per condition. The target was ten per condition.
- The measurement it produced is reported as unfit for purpose in that same file: it counted every
  word not in the prompt, so it scored *whose* and *rather* against a run alongside *hash-keyed*.
  The instruction that won on the number produced the worst prose, because it could not write
  *reset* or *route*.
- Two runs took 32 and 33 minutes against 34-43 seconds for the other four. That was traced, from
  saved timestamps, to the file-editing tool blocking for exactly 600 seconds three times in each
  run. Subtracting the 1,800 blocked seconds leaves the one real timing result: the
  check-every-word instruction cost four to five times the working time.
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  is the redesign — seven conditions, ten runs each, answers saved verbatim so later measurements
  need no rerun. **It has not been run.**

One inconsistency inside the results file: finding 5 still reads "No timing conclusion is
available", while the section above it explains that one became available once the blocked time
was subtracted, and says so explicitly. The two statements contradict each other; the earlier
commit that was meant to fix exactly this kind of contradiction left this one standing.

## Verified

- **The dev-only reset, login and browser-storage routes cannot reach a deployed build.**
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)
  exists and, per the plan entry that describes it, builds what the deployed path builds and
  asserts every `/_dev/*` address answers "not found" while the real addresses answer
  "unauthorized" rather than being absent. The file is on disk; I did not run it. This mattered
  because one of those routes empties all stored data.
- **The Rust test suites exist and are substantial.** I counted 179 test functions across the four
  crates under [backend/](backend/). The page-cleanup plan records an audit that measured 173
  tests passing and 77.57% of lines covered, with every server address having at least one test.
  That audit predates the last few commits.
- **The browser tests exist**: 3 tests in [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and
  14 in [e2e/views.spec.js](e2e/views.spec.js), driving a real browser against a real local server.

## Not verified

- **The real Amazon S3 and DynamoDB storage code has never run against anything.** This is not an
  inference; the files say so in their own opening comments.
  [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) states it has zero test
  coverage and has never run against real or emulated S3.
  [backend/timeline-storage/src/dynamo.rs](backend/timeline-storage/src/dynamo.rs) states its
  key-building logic is unit-tested but that it has never run against a live or local DynamoDB.
  Critique C10 in the migration plan is still marked open and calls this "real, unstarted work" —
  a downloadable local DynamoDB is available and straightforward; no license-compatible local S3
  substitute has been found.
- **Nothing runs the tests automatically.** There is no `.github` directory and no other
  automation; the browser tests run only when someone remembers to run them.
- **Two promises recorded as resolved are not on disk.** Critique C12 in the page-cleanup plan is
  marked `[RESOLVED]` and claims two things. First, that one command now runs every suite — the
  Rust workspace, the browser tests, and the launcher test — and pins the Node version the browser
  tests need. I can find no such command: [scripts/](scripts/) holds `dev-up.sh`, `dev-down.sh`,
  `port-control.sh` and `test-dev-up.sh` only; [.vscode/tasks.json](.vscode/tasks.json) has five
  tasks, four for starting and stopping the dev servers and one for testing the launcher; there is
  no Makefile and no top-level package file. The only Node pinning is an `engines` field in
  [e2e/package.json](e2e/package.json), which is not a command that fails loudly. Second, a check
  for exceptions that are caught and silently dropped, covering both the Rust and the JavaScript —
  no such file exists anywhere in the repository. So that critique's status overstates what
  landed, and "run all tests" still means running three things by hand.
- **Two known defects are recorded and unfixed**, both found while checking the page deletion and
  both scheduled behind the six phases:
  1. Conversation order changes between server restarts, because
     [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
     walks a hash map and never sorts. Observed directly, three runs giving three orders. Visible
     to a user as the conversation list and its colors reshuffling.
  2. Intermittent "Failed to fetch" from the page to a server that was demonstrably answering.
     **Not explained** — worked around by waiting for the server to answer before driving the
     browser. A user hitting this sees only "Is the backend running?" when it is.
- **The reset that was the last piece of work landed in one of the two browser test files.**
  [e2e/views.spec.js](e2e/views.spec.js) calls the reset address before each test;
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) contains no reset and no before-each hook.
  Whether that was intended I cannot tell from the commit, which touched only the first file.
- **Leftover writes to the Claude-artifact storage remain in the page.**
  [timeline.html](timeline.html) still writes a flag cache to `window.storage` inside
  `saveAutoClassificationsToStorage`, guarded by a check that the storage exists. The deletion
  phase listed sweeping this as in-scope. It is tied to the "Classify with AI" button, which the
  plan's own audit calls untested, possibly already dead in this setup, and slated for replacement.
- **Two open design questions await the user, not more work**: whether restoring the last export on
  page load should announce itself (marked resolved as "announced, per your call", so likely
  settled), and how the two tiers of emotion detection — the free pass and the model-based one —
  are named and explained to users. That naming is tabled at the user's own direction; no
  user-facing labels were invented.

## Shortest description of the state

The browser page and the local, in-memory server work together and are covered by tests that
exist and are claimed to pass. Nothing has ever touched real cloud storage, nothing runs the tests
on its own, and the single command that was supposed to run everything was never written. The last
two days of work were not on the product at all but on measuring how these instructions affect the
words used in replies, and that experiment is between its first attempt and its redesign.
```

---

## E-q3-r3

**Variant on disk (not what reached the instance):** E  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is checked versus not

I answered by reading files and git history only. I did not run the tests, so every
"passes" below is something I read in a file, not something I watched happen.

## Where it left off

The last change to the product itself was on 2026-09-29. Everything committed on
2026-09-30 is about the writing rules in [CLAUDE.md](CLAUDE.md), not about the timeline
tool.

Last four product commits, oldest first:

- `da1457a` — the in-memory stores can now be emptied
  ([backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs)).
- `d725cc3` — a new `POST /_dev/reset` route
  ([backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs)),
  plus a test that the router the deployable build uses has no `_dev` routes at all
  ([backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)).
- `d81b89e` — the browser tests call that reset before each test
  ([e2e/views.spec.js:51](e2e/views.spec.js#L51), [:128](e2e/views.spec.js#L128)).
- `7cb86d9` — a plan-document edit marking that concern closed.

All six phases of [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
are marked **Done** in its own table (lines 12-17). The page itself is now 3,110 lines /
119 KB, down from the 66,839 lines / 752 KB that plan measured, so the large deletion of
the embedded word list and sentiment word list did land.

That plan says, at its top, that these were fixes to finish **before** going back to the
server work in [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
— specifically the real Amazon S3 and DynamoDB adapters. So that is the declared next
step, and it has not been started.

Work in progress right now, uncommitted in the working tree: edits to
[CLAUDE.md](CLAUDE.md) and to
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).
That second document is a written-but-not-yet-run experiment comparing seven wordings of
the vocabulary rule; the earlier attempt's results are in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md)
and rest on two runs per condition.

## Checked

- **179** `#[test]` / `#[tokio::test]` markers across the Rust crates. That is a count of
  markers I grepped, not a count of tests I saw pass.
- The most recent line-coverage figure anywhere in the repo is **77.57%**, recorded in
  the quality-of-life plan at the time of its second phase. More tests have landed since.
  Nobody has re-measured it, and I did not.
- Browser tests exist and drive the real local server with a real headless Chrome:
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) (3 tests) and
  [e2e/views.spec.js](e2e/views.spec.js) (13 `test(...)` calls, one of which is generated
  once per analytics view from a list). They cover upload, the calendar, opening a
  conversation, each analytics view, review search/filter/pages, the annotated download,
  on-demand detection and its progress, upload byte progress, hash navigation, and
  session restore.
- [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json) reads
  `{"status": "passed", "failedTests": []}`, timestamped today at 04:14. Three cautions:
  that file is ignored by git, it records only the single most recent local run, and it
  does not say which test files that run included. I cannot tell who or what ran it.
- The `_dev` routes, including the one that erases everything, are absent from the router
  the deployable build constructs — asserted by a real test
  ([backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)),
  not only by reading the source.
- Hand-run checks recorded in [backend/README.md:120-134](backend/README.md#L120-L134):
  the compiled function was driven through a local emulation of the AWS Lambda runtime
  with genuine API-Gateway-shaped events; four request shapes came back correct. That
  session is described there as manual and not captured as an automated test.

## Not checked

- **The real Amazon S3 and DynamoDB code has never talked to anything.**
  [backend/timeline-storage/src/s3.rs:1-9](backend/timeline-storage/src/s3.rs#L1-L9) says
  so in its own opening comment, and
  [backend/README.md:135-141](backend/README.md#L135-L141) repeats it: no `send()` call in
  either file has reached AWS or a local stand-in.
- **Worse than untested — unconnected, and I did not find this written down anywhere.**
  Reading [backend/timeline-api/src/main.rs:114-120](backend/timeline-api/src/main.rs#L114-L120):
  the branch that runs under AWS Lambda calls the same `build_local_state()` as local
  development, which builds the in-memory stores. A grep for `S3ObjectStore` and the two
  DynamoDB table types finds them referenced nowhere outside their own files and comments.
  So the only runnable program in this repo never constructs them. A deploy of the current
  template would come up with no persistence at all. The comment at the top of `main.rs`
  describes only *local* mode as in-memory, and I found no plan or README entry stating
  that the deployable path is in-memory too.
- [infra/template.yaml:14-17](infra/template.yaml#L14-L17) says outright it has never been
  validated or deployed, and lines 9-12 say it deliberately omits the upload-processing
  function and the export route's resources.
- Nothing has ever been checked against real Amazon Cognito tokens — only a throwaway
  key pair generated at startup.
- The paid classification tier, the $5 charge, and the hardening pass (the migration
  plan's V3, V4 and V5 sections) are not started.
- **There is no continuous integration.** No `.github` directory exists. The tests run
  only when a person remembers to run them.
- The silent-failure check that [CLAUDE.md](CLAUDE.md) refers to as
  `tests/test_no_unhandled_exceptions.py` does not exist in this repo, and there is no
  Python here at all.
- The file-length check
  ([backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs))
  only looks at `.rs` files under `backend/`, so [timeline.html](timeline.html) is
  invisible to it.

## Three places where a document claims something the code does not show

1. **The page still tells users nothing is uploaded.**
   [timeline.html:743](timeline.html#L743) reads "This page reads your exported Claude
   conversation data directly in your browser — nothing is uploaded anywhere." The page
   now uploads the export to the backend. The quality-of-life plan's third phase lists
   correcting this line as mandatory ("Must be corrected regardless of the rest of this
   phase"), and that phase is marked Done. `git log -S` on that sentence returns only the
   initial commit, so it has never been edited.
2. **"One command runs every suite" — I could not find that command.** Entry C12 of the
   quality-of-life plan is tagged `[RESOLVED]` and says one command now runs the Rust
   tests, the browser tests and the launcher test, and pins the Node version. What is in
   the tree: [scripts/](scripts/) holds `dev-up.sh`, `dev-down.sh`, `port-control.sh` and
   `test-dev-up.sh`; [e2e/package.json](e2e/package.json) has `"test": "playwright test"`;
   there is no Makefile, no cargo alias file, and no such task in
   [.vscode/tasks.json](.vscode/tasks.json). Either it lives somewhere I did not look, or
   that entry was marked resolved in the plan without the change landing. The same entry
   also asks for the silent-failure check, which is confirmed missing above.
3. **Leftover use of the artifact-only storage interface.** The third phase said to sweep
   `window.storage` out. One use remains at
   [timeline.html:1611](timeline.html#L1611). This one is explained in place
   ([timeline.html:1580-1601](timeline.html#L1580-L1601)): it checkpoints an in-progress
   "Classify with AI" run, which only works inside a Claude artifact anyway. Documented,
   narrower than before, but not swept.

## Open items already recorded, carried forward

From the quality-of-life plan's "Known defects" section (both found while checking the
third phase, both scheduled behind it by the user):

- **Conversation order changes between backend restarts.** `list_for_user` in
  [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
  walks a hash map and returns without sorting. The plan records three runs of the same
  unchanged page producing three different orders. Visible to a user as the conversation
  list and its colours reshuffling.
- **Occasional "Failed to fetch" from the page to a backend that is demonstrably
  answering.** Worked around by waiting for the port first; not explained.

Two entries in that plan's critique log are tagged `[OPEN]` and both look stale to me:

- C1 (the freeze while the page parses the export) says its trigger is a measurement that
  could not be taken until the big deletion landed. That deletion has landed. I found no
  such measurement recorded.
- C8 asks whether restoring the last session should be silent or announced. The phase
  table already says "restore is announced, per your call", and there is a test for the
  announcement at [e2e/views.spec.js:435](e2e/views.spec.js#L435). The question appears
  settled while the entry still reads open.

Tabled by the user, not by me: how the two detection tiers are named and explained to
users.
```

---

## G1-q1-r1

**Variant on disk (not what reached the instance):** G1  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and the standard way is already half-employed in this repo. Everything below is read in
source; I ran nothing.

## The standard mechanism, per layer

**1. Browser tests, server-side state — Playwright's per-test setup hook.**
This is the standard answer and it is already wired in one of the two test files:
[e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) is a `test.beforeEach` that calls
`resetBackend()` ([e2e/views.spec.js:50](e2e/views.spec.js#L50)), which posts to `/_dev/reset`.
The endpoint is [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs);
it empties the four in-memory stores wired at
[backend/timeline-api/src/main.rs:92-97](backend/timeline-api/src/main.rs#L92-L97) (object store,
conversation summaries, message flags, upload outcomes). The capability is a trait at
[backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24),
implemented only by the four in-memory stand-ins, so there is no path from it to a real S3 bucket
or DynamoDB table.

So for the shared-server problem there is nothing to invent. There is a gap to close, though:

**[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no reset hook.** It has only
`test.beforeAll` ([line 53](e2e/upload-flow.spec.js#L53)) and `test.afterAll`
([line 76](e2e/upload-flow.spec.js#L76)). It also never fills `#devLoginSub`, so all three of its
tests run as the page's default login name `alice`
([timeline.html:752](timeline.html#L752) sets `value="alice"`) and accumulate each other's uploads
and flags. That weakens its own third assertion: "confirming a flag … persists through a reload"
([line 130](e2e/upload-flow.spec.js#L130)) ends by asserting `OVERRIDES` is non-empty, which data
left behind by the file's first test could also satisfy.

**2. Making the reset impossible to forget — one shared setup module.**
Playwright's standard way to share per-test setup across spec files is `test.extend`: a module
that re-exports an extended `test`, with a setup entry marked to run automatically for every test.
Each spec file then imports `test` from that module instead of from `@playwright/test`, and the
reset happens whether or not whoever writes the next test remembers it. That same module is the
natural home for the ~45 lines the two files currently duplicate: `BACKEND_DIR`, `TIMELINE_HTML`,
`FIXTURE`, `API_BASE`, `waitForPort`, and the server spawn.

Note what is *not* the right tool: `globalSetup` in
[e2e/playwright.config.js](e2e/playwright.config.js) runs once per whole run, not once per test.
It is the right place to start the server; it cannot give each test a clean slate.

**3. Rust tests — already using the stronger standard.**
They build fresh state per test rather than resetting shared state: `test_state()` at
[backend/timeline-api/tests/app.rs:38](backend/timeline-api/tests/app.rs#L38) and `test_router()`
at [backend/timeline-api/tests/dev_routes.rs:34](backend/timeline-api/tests/dev_routes.rs#L34).
Nothing to change. `dev_routes.rs` even passes an empty reset list deliberately
([line 62](backend/timeline-api/tests/dev_routes.rs#L62)) so a later test that *does* reset fails
loudly instead of quietly doing nothing.

**4. Browser-side state — already neutral by default.**
Playwright gives each test its own browser context, so the two `localStorage` keys the page writes
— `timeline_api_base` ([timeline.html:1049](timeline.html#L1049)) and `timeline_dev_sub`
([timeline.html:1075](timeline.html#L1075)) — start empty in every test. Nothing to add unless
someone later sets `storageState` in the config.

## One inconsistency worth fixing in the same pass

The two test files disagree on server lifecycle, and only the newer one looks right:

- [e2e/views.spec.js:90-125](e2e/views.spec.js#L90-L125) reuses a server already listening on port
  3000, spawns `detached: true` when it has to start one, and kills the whole process group at
  [line 132](e2e/views.spec.js#L132) — its own comment at
  [line 112](e2e/views.spec.js#L112) says the process group exists so the kill takes the real
  server down "rather than orphaning it holding the port".
- [e2e/upload-flow.spec.js:53-78](e2e/upload-flow.spec.js#L53-L78) always spawns, not detached,
  and signals only the `cargo` wrapper with `serverProcess.kill('SIGTERM')`.

Playwright runs spec files in alphabetical order with `workers: 1`, so `upload-flow` runs first and
`views` reuses whatever it leaves behind. If the upload-flow teardown does orphan the server, the
port stays held. I have not run this to confirm the orphaning; the claim I am confident about is
only that the two files handle the same job differently and that views.spec.js's comment says the
difference matters.

## Recommendation

1. Smallest: add `test.beforeEach(resetBackend)` to
   [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js), and a distinct login name per test there too.
2. Better: extract the shared setup — server start/reuse, teardown by process group, and the
   automatic per-test reset — into one module both spec files import via `test.extend`. That is the
   standard shape, it deletes the duplication, and it removes the "someone forgot the hook" failure
   mode rather than fixing one instance of it.

I have not written any code for either.
```

---

## G1-q1-r2

**Variant on disk (not what reached the instance):** G1  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and most of it is already in this repo — the standard shape is (1) a test-only HTTP route
that empties the server's state, plus (2) the test framework's own per-test setup hook calling it
before every test. That is the usual recommendation for browser tests driving a real server
(Playwright and Cypress both steer you to a reset/seed route rather than to careful ordering).

## What already exists here

- `POST /_dev/reset` — [backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24),
  registered at [backend/timeline-api/src/app.rs:55](backend/timeline-api/src/app.rs#L55). It empties
  every store handed to it.
- It can only reach the in-memory stores, because the emptying ability is a separate trait
  implemented only by them — [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24).
  So there is no code path by which it could clear a real S3 bucket or DynamoDB table.
- [e2e/views.spec.js:50](e2e/views.spec.js#L50) wraps it in a `resetBackend()` call and
  [e2e/views.spec.js:128](e2e/views.spec.js#L128) invokes it from `test.beforeEach`.

So the answer to "is there a standard way" is: this is it, and the piece that is missing is not a
mechanism but consistent application of the one already built.

## Three standard mechanisms that would make it apply everywhere

1. **Playwright's `test.extend` with an always-on per-test entry** (Playwright's own word for these
   is a fixture; declared with `{ auto: true }`). Today the reset is a local function plus a
   `beforeEach` inside a single spec file, so every new spec file has to remember to repeat both.
   Exporting a shared `test` from one small module in [e2e/](e2e/) — one that resets before handing
   the test a page — makes the clean slate the default that a file gets by importing `test`, rather
   than per-file discipline. This is the standard answer to "reset must be unforgettable".

2. **`webServer` in [e2e/playwright.config.js](e2e/playwright.config.js)** — Playwright's built-in
   server lifecycle (a command, a URL to poll, `reuseExistingServer`, a timeout). Right now each
   spec file hand-rolls spawn + port polling + kill: ~40 near-identical lines in
   [e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53) and
   [e2e/views.spec.js:88](e2e/views.spec.js#L88), including a "did I start it?" flag so one file does
   not stop a server the other started. Giving the config ownership of the server removes both
   copies and the sharing question with them.

3. **A fresh instance per test, where you can afford one.** This is the stronger form of the same
   idea and the Rust side already does it: each integration test builds its own router over its own
   new in-memory stores ([backend/timeline-api/tests/dev_routes.rs:34](backend/timeline-api/tests/dev_routes.rs#L34)),
   so there is no shared state to reset — note the deliberately empty reset list at
   [backend/timeline-api/tests/dev_routes.rs:62](backend/timeline-api/tests/dev_routes.rs#L62). Only
   the browser tests need a reset route, because they cannot spawn a server per test at a
   ~30-second-per-file startup cost.

## One gap I read in the code (read, not measured — I did not run the tests)

[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never calls the reset route: it has
`test.beforeAll` and `test.afterAll` and no `beforeEach`. With `workers: 1`
([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)) and alphabetical file order, that file
runs first and normally gets a server it started itself, so a single run looks clean.

What I read that makes me doubt this holds across runs: its teardown is
`serverProcess.kill('SIGTERM')` at [e2e/upload-flow.spec.js:77](e2e/upload-flow.spec.js#L77), sent to
the `cargo` process, spawned without `detached`. `cargo run` does not forward that signal to the
binary it launched, so the `timeline-api` process can survive holding port 3000 — the other file
spawns into its own process group and signals the group precisely to avoid this
([e2e/views.spec.js:88](e2e/views.spec.js#L88) onward). If a server does survive, the next run's
`beforeAll` finds port 3000 answering, reuses it, and the upload-flow tests then run against the
previous run's data with no reset. That is an inference from reading the two teardowns side by side,
not something I observed; checking it costs one `ss -ltnp | grep 3000` after `npm test` exits.

## What a reset route cannot cover

The real S3 and DynamoDB adapters deliberately do not implement the emptying trait, so when the
real-adapter tests get built this route will not help them. The standard approaches there are a
per-test bucket name or table-name prefix (isolate instead of erase), or explicit teardown that
deletes what the test created. Worth deciding then, not now.

Also worth keeping in mind for any reset route: it must be structurally absent from the production
build, not merely unrouted by convention. That is already asserted, in
[backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs), in both
directions — every `_dev` path 404s, and the real routes are still present.

## Suggested order, if you want one

Apply the reset in [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) first (smallest change, closes
the actual hole), then move the reset into a shared `test` the spec files import, then hand the
server lifecycle to `webServer`. I have not written any of this — say the word and I will put it in
a plan under [docs/plans/](docs/plans/) first.
```

---

## G1-q1-r3

**Variant on disk (not what reached the instance):** G1  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and the repo already contains most of it. Read-only answer: I read the files below and ran no tests.

## There are two standard approaches, and this repo uses both already

**1. Build fresh state per test.** The Rust tests do this: each one calls its own `test_router()`, which
constructs brand-new in-memory stores, so there is no starting state to reset — see
[backend/timeline-api/tests/export.rs:34-65](backend/timeline-api/tests/export.rs#L34-L65) (same helper
copied in [backend/timeline-api/tests/detect.rs:38](backend/timeline-api/tests/detect.rs#L38) and
[backend/timeline-api/tests/dev_routes.rs:34](backend/timeline-api/tests/dev_routes.rs#L34)). This is the
better option whenever it is affordable, because nothing can leak by construction.

**2. Reset a shared instance through a test-only endpoint.** The browser tests can't use option 1 —
they drive one long-lived `cargo run -p timeline-api` over HTTP — so the repo has the standard
test-only reset hook: `POST /_dev/reset` at
[backend/timeline-api/src/routes/dev_reset.rs:24-29](backend/timeline-api/src/routes/dev_reset.rs#L24-L29),
backed by the `Resettable` trait at
[backend/timeline-storage/src/memory/resettable.rs:24-28](backend/timeline-storage/src/memory/resettable.rs#L24-L28),
with the list of stores it empties wired up at
[backend/timeline-api/src/main.rs:92-97](backend/timeline-api/src/main.rs#L92-L97) (all four stores are in
that list). It is registered only in the local-dev router
([backend/timeline-api/src/app.rs:55](backend/timeline-api/src/app.rs#L55)) and there is a test asserting it
stays out of the Lambda build and that it actually empties a store
([backend/timeline-api/tests/lambda_router.rs:104](backend/timeline-api/tests/lambda_router.rs#L104)).

So the answer to "is there a standard way" is: the mechanism exists and is already built. What is
missing is having it applied *uniformly and unforgettably*.

## The gap I can see by reading

[e2e/views.spec.js:50](e2e/views.spec.js#L50) defines `resetBackend()` and
[e2e/views.spec.js:128](e2e/views.spec.js#L128) calls it in `test.beforeEach`.
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) does **not** reset at all — it has a `beforeAll`
([line 53](e2e/upload-flow.spec.js#L53)) and an `afterAll` ([line 76](e2e/upload-flow.spec.js#L76)) and
nothing per-test. It also never fills `#devLoginSub`, so all three of its tests run as the page's default
login name `alice` ([timeline.html:752](timeline.html#L752)) against a store that accumulates.

That matters for one assertion specifically. The third test
([e2e/upload-flow.spec.js:130](e2e/upload-flow.spec.js#L130)) approves a flag, re-uploads, and asserts
`Object.keys(OVERRIDES).length > 0`. That assertion passes if *any* override exists for `alice` — including
one left by an earlier run. Today the server is started fresh by that file's `beforeAll`, so I don't think
it is reachable right now; it becomes reachable the moment the suite reuses an already-running server or is
run twice against one long-lived `cargo run`. A reset there makes the test *sounder*, not just tidier.

## Standard Playwright machinery that would make the reset unforgettable

- **Attach the reset to every test automatically instead of per-file `beforeEach`.** Playwright's
  `test.extend` with an always-on setup step (`{ auto: true }`) in a shared file that both spec files import
  their `test` from. A new spec file then cannot forget the reset, which is exactly how
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) came to be missing it. A per-test reset must not go in
  `globalSetup` — that runs once for the whole run.
- **Let the config start and stop the server.** Playwright's built-in `webServer` option in
  [e2e/playwright.config.js](e2e/playwright.config.js) replaces the duplicated `spawn` + `waitForPort` +
  `kill` blocks in both spec files, and removes the hazard the comment at
  [e2e/views.spec.js:90-100](e2e/views.spec.js#L90-L100) is working around by hand (two files each trying to
  bind port 3000, the loser silently testing against the winner's server). Point `url` at
  `http://127.0.0.1:3000/conversations`, which answers 401 unauthenticated
  ([backend/timeline-api/src/auth_extractor.rs:32](backend/timeline-api/src/auth_extractor.rs#L32)); Playwright's
  documented readiness check accepts 401 as "listening" — worth confirming against the installed 1.63 docs
  rather than taking my word. Give it a generous `timeout`, since the first start pays for a `cargo` compile.
- **Browser-side state needs nothing extra.** Playwright's documented default gives each test its own fresh
  browser context, so the two `localStorage` keys the page writes — `timeline_api_base` and
  `timeline_dev_sub` ([timeline.html:1049-1052](timeline.html#L1049-L1052),
  [timeline.html:1075](timeline.html#L1075)) — start empty every test, while surviving the in-test
  `page.reload()` that the session-restore tests depend on. If a test ever needs a *preloaded* browser state,
  that is what `storageState` is for. (Documented default behavior, not something I measured here.)

## Two things to keep in mind

- **Reset-a-shared-server and multiple workers are in tension.** One test's reset wipes another's data.
  [e2e/playwright.config.js:14](e2e/playwright.config.js#L14) pins `workers: 1`, which is what makes the
  current reset safe. If that ever changes, the standard fix is one server per worker on a port derived from
  the worker index, not a shared server plus resets.
- **The three Rust `test_router()` helpers pass `resettable: Arc::new(vec![])`.** The comment at
  [backend/timeline-api/tests/export.rs:58-62](backend/timeline-api/tests/export.rs#L58-L62) says a later test
  that resets will "fail loudly" — precisely, the reset call itself still returns 204 with zero stores to
  empty, so the loudness comes from whatever that test asserts about the data afterwards, not from the reset
  call. Also worth folding those three near-identical copies into one shared `tests/common/mod.rs`, which is
  the standard way to share a helper between Rust integration test binaries.
```

---

## G1-q2-r1

**Variant on disk (not what reached the instance):** G1  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Read from the source only — I ran nothing and wrote no code.

## Short answer

In the code as it stands, it no longer always detects. The backend scan runs only when you
tick a box on the load screen, and that box starts unticked. What you describe was true of an
earlier state of the code, and it was changed in commit 6c7d9a9, "Move detection out of upload
into a user-triggered POST /detect".

And yes, it is a different thing from the "Classify with AI" button: different machine, different
judge, different money, and only two of the three tags overlap. You are right that this confuses
users, and it still will, because almost nothing in the interface says any of it.

## Why it used to run on every upload

Not an oversight. [docs/plans/2026-09-09-rust-aws-backend-migration.md:32](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32)
records a decision you confirmed: a free pass (a dictionary check for ALL-CAPS plus keyword and
sentiment scoring for criticism and anger, "~$0 marginal cost") "always available", against one
paid Bedrock-quality pass per $5. What got built read "always available" as "already computed at
upload, whether or not anyone asked". Those are two different claims, and
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
(Phase 4) says so and specifies the change.

## What the code does now

- [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98) says in
  its own words: "This does not compute flags." Upload parses, dedups, stores summaries, records
  the outcome, and stops.
- The scan lives in [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs),
  registered at [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27). It takes
  an offset and a limit and answers with a next offset, so the page can ask for it a few
  conversations at a time and move a progress bar that means something. The shared scoring function
  is [backend/timeline-api/src/processing.rs:84](backend/timeline-api/src/processing.rs#L84), and
  the route is now its only caller.
- On the page: the box is at [timeline.html:756](timeline.html#L756) with no `checked` attribute,
  read at [timeline.html:1334](timeline.html#L1334), and acted on only inside the `if` at
  [timeline.html:1384](timeline.html#L1384). The loop that drives it is
  [timeline.html:1303](timeline.html#L1303).

So a freshly uploaded export renders with no automatic tags at all until you ask for them. Phase 4
calls that an intended change, not a missing write.

## How the two actually differ

|                        | The box on the load screen | The "Classify with AI" button |
|---|---|---|
| Where the work happens | On the server, in Rust      | In your browser, calling `api.anthropic.com` ([timeline.html:1745](timeline.html#L1745)) |
| What decides           | Dictionary check, keyword lists, sentiment scoring | `claude-sonnet-4-6` ([timeline.html:1691](timeline.html#L1691)), 30 messages per call, each message sent with the preceding Claude reply for context |
| Which tags             | ALL-CAPS, criticism, anger  | Criticism and anger only — never ALL-CAPS ([timeline.html:1850](timeline.html#L1850)) |
| Money                  | None                        | Real tokens; a dialog says so before it starts ([timeline.html:1817](timeline.html#L1817)) |
| Where the answer lives | Stored per message in the backend, and written into the export the page reads back ([backend/timeline-api/src/routes/export.rs:88](backend/timeline-api/src/routes/export.rs#L88)) | In memory for this session, plus the file you download. Nothing goes back to the server ([timeline.html:1598](timeline.html#L1598)) |
| Works when             | Always                      | Only while the page is running live as a Claude artifact; a local copy of the file cannot reach the API ([timeline.html:840](timeline.html#L840)) |
| Marker in the Review tab | `auto`                    | `AI` ([timeline.html:2479](timeline.html#L2479)) |

Both write only the automatic values and leave anything you confirmed yourself alone. That part is
consistent between them.

## Five places a user gets lost

1. **Two triggers, two screens, neither mentions the other.** A box before the upload
   ([timeline.html:758-765](timeline.html#L758-L765)) and a button in the Review tab
   ([timeline.html:835-844](timeline.html#L835-L844)). Nothing links them.
2. **The button's own text assumes the box was ticked.** It offers to be "more accurate than the
   keyword/sentiment heuristic below" ([timeline.html:838](timeline.html#L838)). Leave the box
   unticked and there is no result below to be more accurate than — the button is then the only
   thing producing automatic tags, not an improvement on one.
3. **The two results are told apart only by a three-letter marker.** `auto` against `AI`, with a
   hover title as the sole explanation ([timeline.html:2462](timeline.html#L2462)). Same rows, same
   boxes, same "this only overwrites automatic tags" promise.
4. **The scan cannot be run later.** Its only trigger is the load screen, so the way to get
   automatic tags after the fact is to upload the file again. Phase 4 notes a separate button would
   be reasonable and deliberately does not specify one.
5. **Uploading a saved file quietly relabels the AI's answers as the cheap ones.**
   [backend/timeline-api/src/routes/export.rs:88-103](backend/timeline-api/src/routes/export.rs#L88-L103)
   overwrites `_claude_timeline_auto` whenever a stored record exists for that message, and writes
   `"source": "heuristic"` unconditionally. A record exists not only after a scan but also after you
   confirm any flag: both writers create the record if it is absent, with the automatic part blank
   ([backend/timeline-storage/src/memory/message_flags.rs:101](backend/timeline-storage/src/memory/message_flags.rs#L101);
   the DynamoDB one at [backend/timeline-storage/src/dynamo/message_flags_table.rs:244](backend/timeline-storage/src/dynamo/message_flags_table.rs#L244)
   is an `update_item`, which likewise creates the item). So: pay for the AI pass, save the file,
   upload it again, and messages you had confirmed come back with all three automatic tags false and
   the marker reading `auto`. I read this path; I did not run it.

## One more thing, since it undercuts any explanation of the difference

[timeline.html:743](timeline.html#L743) still tells the user "nothing is uploaded anywhere". The
file is uploaded, and the scan is server-side work on it. Phase 3 of the same plan flags this copy
as false and says it must be corrected regardless of the rest; it has not been. No wording about
two kinds of detection can land while the first paragraph denies that anything leaves the browser.

## What I would suggest

Phase 4 records that you have tabled how the two are named and explained, and deliberately invents
no labels. Taking that at face value, the only thing I would raise now is item 5 — the relabelling
is a data loss, not a wording problem, and it does not depend on how the two are eventually
named — and item 4, because "re-upload to get tags" is the reason a user would reach for the paid
button without meaning to.
```

---

## G1-q2-r2

**Variant on disk (not what reached the instance):** G1  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
## Short answer: it no longer does

I read the code before answering, and the behaviour you are describing has already been
changed. The backend does **not** always detect any more. Two commits did it:
`6c7d9a9` "Move detection out of upload into a user-triggered POST /detect" and
`64996c5` "Add upload progress, on-demand detection, and in-page navigation".

What I read, so you can check me:

- [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98) —
  `process_upload` carries the comment "**This does not compute flags.**" Reading the body at
  [processing.rs:106-156](backend/timeline-api/src/processing.rs#L106-L156), it parses, dedups,
  writes one summary per conversation, and records the outcome. No call to
  `set_auto_flags` anywhere in it.
- [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs) —
  the scan now lives behind `POST /detect`, one page of conversations per request
  ([detect.rs:43](backend/timeline-api/src/routes/detect.rs#L43) sets the default page at 5).
- [timeline.html:756](timeline.html#L756) — a checkbox on the load screen, **unchecked by
  default**. [timeline.html:1334](timeline.html#L1334) reads it and
  [timeline.html:1384-1387](timeline.html#L1384-L1387) is the only place `runDetectionPass`
  ([timeline.html:1303](timeline.html#L1303)) is ever called.
- [timeline.html:999-1001](timeline.html#L999-L1001) — the page itself does no detection at
  all now, and a message with no stored automatic flags "simply has none -- which is the
  normal state until the user asks for a detection pass."
- Two of the browser tests already hold this in place:
  [e2e/views.spec.js:318](e2e/views.spec.js#L318) "uploading without asking for detection
  produces no flags at all" and [e2e/views.spec.js:331](e2e/views.spec.js#L331) "the
  detection pass reports progress while it runs". I did not run them — you asked me to read
  only — so what I can tell you is that the tests exist and what they assert, not that they
  currently pass.

## Why it used to always run

Not an oversight. [docs/plans/2026-09-09-rust-aws-backend-migration.md:32-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32-L33)
records a confirmed decision: a free tier (dictionary ALL-CAPS check plus keyword and
sentiment criticism-anger, "~$0 marginal cost") that is "**always available**", against one
paid Bedrock-quality pass per $5. What got built read "always available" as "already
computed, at upload, whether or not anyone asked." Those are two different claims, and
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
onward is the phase that separated them.

## Yes, it is a different thing from "Classify with AI"

They are two passes over the same three flags, and nothing in the interface explains the
relationship. Read side by side:

| | the load-screen checkbox | the "Classify with AI" button |
|---|---|---|
| Where it runs | the backend, in Rust ([detect.rs:126](backend/timeline-api/src/routes/detect.rs#L126)) | the browser, calling `api.anthropic.com` straight from the page ([timeline.html:1745](timeline.html#L1745)) |
| How it decides | fixed word lists, a dictionary check for ALL-CAPS, sentiment scoring ([processing.rs:84-90](backend/timeline-api/src/processing.rs#L84-L90)) | asks Claude Sonnet to judge each message, with the preceding Claude reply for context ([timeline.html:1691](timeline.html#L1691), [:1711](timeline.html#L1711)) |
| Which flags | all three: ALL-CAPS, critical, angry | **only two**: critical and angry. It never touches ALL-CAPS ([timeline.html:1848-1850](timeline.html#L1848-L1850)) |
| What it costs | no per-message charge | real tokens, 30 messages per call ([timeline.html:1690](timeline.html#L1690)) |
| Does the result survive a reload | yes — written to server storage and embedded in the export as `"source": "heuristic"` ([export.rs:99](backend/timeline-api/src/routes/export.rs#L99)) | **no** — it updates the page's own copy and writes a cache the load path no longer reads back ([timeline.html:1591-1600](timeline.html#L1591-L1600)). Nothing is sent to the backend |
| Where it works | anywhere the backend is reachable | only while the page is running live as a Claude artifact ([timeline.html:841](timeline.html#L841)) |

Both share one rule, and it holds: neither touches a flag you confirmed yourself. The
checkbox copy says so at [timeline.html:763](timeline.html#L763), the button's confirm box at
[timeline.html:1818](timeline.html#L1818), and the code keeps automatic and confirmed values
in two separate fields ([timeline.html:988-995](timeline.html#L988-L995)).

## Where users will still be confused, and why

You are right that this will confuse people, and landing the on-demand change did not fix
that. The Phase 4 write-up says outright that naming and explaining the two passes to users
was **tabled** for a separate design, so the interface today has these gaps:

1. **The two triggers sit in two different places and never mention each other.** One is a
   checkbox you can only reach before the file is loaded; the other is a button in the Review
   tab. Nothing tells you they write to the same three flags.
2. **The only thing distinguishing a result is a three-letter marker.** The Review table
   prints `auto`, `AI`, or `you` ([timeline.html:2479](timeline.html#L2479)); the fuller
   wording is hidden in a `title` that appears on hover
   ([timeline.html:2462](timeline.html#L2462)), and there is no key for it on that tab.
3. **The button's own copy points at something that is not there.** It offers to beat "the
   keyword/sentiment heuristic **below**" ([timeline.html:838](timeline.html#L838)). That
   pass no longer runs anywhere near the Review tab, and if the checkbox was left unticked it
   did not run at all — so the thing being compared against may not exist.
4. **The failure message can say something false.** On a failed batch the label reads that
   those messages "may still show the heuristic's tags"
   ([timeline.html:1898](timeline.html#L1898)). With the checkbox unticked they have no tags
   at all.
5. **You cannot ask for the cheap scan after the fact.** `runDetectionPass` has exactly one
   caller, inside the load handler. If you did not tick the box, the only way to get those
   flags is to upload the file again. Phase 4 says a separate button to run or re-run it is
   reasonable but was deliberately not specified.
6. **"I scanned and found nothing" looks identical to "I never scanned."** The download at
   [timeline.html:1924-1928](timeline.html#L1924-L1928) always writes
   `_claude_timeline_auto`, so a file saved without ever running detection comes back with
   three `false` values recorded as though a pass had looked. The page's own load path does
   keep a `none` marker for this case
   ([timeline.html:1005](timeline.html#L1005)) — it is the saved file that loses the
   distinction, and only for messages the page had in memory.

Items 3, 4 and 6 I would call defects rather than open design questions: they are wording and
saved data that contradict how the code now behaves. Items 1, 2 and 5 are the design work
that was tabled.

One thing I am not claiming: how long either pass takes. Nothing I read measures it, and both
the route's own comment ([detect.rs:4-8](backend/timeline-api/src/routes/detect.rs#L4-L8))
and the plan say the same — that is why both passes show progress rather than a frozen
screen.
```

---

## G1-q2-r3

**Variant on disk (not what reached the instance):** G1  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Read-only answer. I opened the backend source, the page, and the two design documents; I ran
nothing.

## Short version

It no longer always does, as the code stands today. It used to, and the reason it used to is
written down. And you are right that the two triggers are easy to mix up: the interface still does
not tell anyone how the load-screen tick box relates to the "Classify with AI" button, and one
document says plainly that naming and explaining the pair was set aside for later.

## 1. What the code does now

Uploading a file stores it and lists its conversations. It computes no flags at all. The function
that handles an upload says so in its own comment — [backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98):
"**This does not compute flags.**"

The word-list-and-sentiment pass lives in a separate request, `POST /detect`
([backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)). The only
thing in the whole backend that calls it is that one route; I grepped for the shared function
`heuristic_flags` and its only caller outside the tests is
[backend/timeline-api/src/routes/detect.rs:126](backend/timeline-api/src/routes/detect.rs#L126).

The page asks for it only when a tick box on the load screen is ticked, and that box has no
`checked` attribute, so it starts empty — [timeline.html:756](timeline.html#L756), read by
[timeline.html:1334](timeline.html#L1334), acted on at [timeline.html:1381](timeline.html#L1381)
under the comment "Only if asked."

The change landed in commit 6c7d9a9, "Move detection out of upload into a user-triggered
POST /detect", whose message states the principle you were pressing on: "Uploading a file is not
consent to run a pass over every message in it."

Two of the browser tests assert this from outside: "uploading without asking for detection produces
no flags at all" ([e2e/views.spec.js:318](e2e/views.spec.js#L318)) and "the detection pass reports
progress while it runs" ([e2e/views.spec.js:331](e2e/views.spec.js#L331)). I read them; I did not
run them, so what I can report is that the assertions exist and say that, not that they passed
today.

## 2. Why it used to always run

Not an accident. The migration plan settled a pricing shape in which the free word-list tier is
"always available", against one paid, better-quality pass per five dollars —
[docs/plans/2026-09-09-rust-aws-backend-migration.md:29](docs/plans/2026-09-09-rust-aws-backend-migration.md#L29).
What got built turned "always available" into "already computed at upload, whether or not anyone
asked". Those are different promises, and the later plan says so in the section written to answer
this exact question — [docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
onward. That section also refuses the speed excuse: nobody measured how long the pass takes, so the
reasons for the change are control and clarity, not cost.

## 3. Is it different from "Classify with AI"? Yes — in five ways

| | The load-screen scan (`POST /detect`) | The "Classify with AI" button |
|---|---|---|
| What judges the message | Word lists, an ALL-CAPS check against a dictionary so that IRS and DARPA are skipped, and a sentiment score, all in Rust ([backend/timeline-core/src/flags/](backend/timeline-core/src/flags/): `caps.rs`, `criticism.rs`, `anger.rs`) | Each message, plus the Claude reply it answered, is sent to Claude Sonnet, which decides ([timeline.html:1685](timeline.html#L1685) onward) |
| Where the work happens | On the server | In the browser, calling `api.anthropic.com` directly — which only works while the page is running live as a Claude artifact, never from a downloaded copy of the file ([timeline-project-decisions.md:284](timeline-project-decisions.md#L284)) |
| What it decides | Three things: ALL-CAPS emphasis, criticism, anger | Two: criticism and anger. It never touches the ALL-CAPS value |
| What it costs | Nothing per run | Tokens; the decisions document estimated about three dollars at Sonnet for a 2,200-message export |
| When you can start it | Only at upload time | Any time, from the Review tab |

So they are genuinely two different mechanisms, with the second meant to be the better judge of
sarcasm, negation and mixed tone that a fixed word list reads wrong — which is why it exists at all
([timeline-project-decisions.md:278](timeline-project-decisions.md#L278)).

## 4. Where they collide, and why users will still be confused

**Both write the same values.** Neither touches a flag you confirmed yourself — that separation is
enforced by giving each route a different narrow capability
([backend/timeline-core/src/ports/message_flags.rs](backend/timeline-core/src/ports/message_flags.rs)).
But they both aim at the *automatic* value, so running the button after the scan replaces the
scan's answers for criticism and anger, silently.

**One of the two forgets which produced a value.** The stored record on the server carries only
three true/false values and no note of where they came from —
[backend/timeline-core/src/ports/message_flags.rs:33](backend/timeline-core/src/ports/message_flags.rs#L33).
The page does keep a note in memory, `auto_source`, set to `'llm'` when the button produced a value
([timeline.html:1850](timeline.html#L1850)), and the Review table shows it as "AI" instead of
"auto" with hover text ([timeline.html:2462](timeline.html#L2462),
[timeline.html:2479](timeline.html#L2479)). But the button's results are never sent to the server
at all, and the server's export writes `"source": "heuristic"` as a hard-coded string for every
message ([backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)).
Read together: click the button, reload the page from the server, and Claude's judgements are gone,
with every remaining tag labelled as the word list's. The only places they survive are the
Claude-artifact storage the page writes to during the run and the annotated file you download
([timeline.html:1928](timeline.html#L1928)).

**You cannot run the free scan later.** The tick box is the only trigger. Skip it, and the only way
to get those tags is to upload the export again. The plan admits this and calls a Review-tab button
for it "a reasonable addition but ... **not** specified here"
([docs/plans/2026-09-28-frontend-quality-of-life.md:243](docs/plans/2026-09-28-frontend-quality-of-life.md#L243)).
That asymmetry is itself confusing: one of the two sits in the Review tab as a button, the other is
reachable only from the screen you have already left.

**Nothing names the pair.** The tick box says "After uploading, scan for likely ALL-CAPS emphasis,
criticism of Claude, and anger" ([timeline.html:758](timeline.html#L758)); the button says "Classify
with AI" ([timeline.html:844](timeline.html#L844)). Neither mentions the other, and no text says
that the second overwrites what the first decided. This is open on purpose: the plan states that
how the two are named and explained was set aside by you to be designed separately, so that phase
"deliberately does not invent user-facing labels, does not pair the two triggers in the interface"
([docs/plans/2026-09-28-frontend-quality-of-life.md:269](docs/plans/2026-09-28-frontend-quality-of-life.md#L269)).

## 5. What I would change, if you want the confusion closed

1. Put both triggers in one place in the Review tab, side by side, so a reader sees two ways of
   producing the same three values and picks one. The free one then also becomes re-runnable, which
   it is not today.
2. Say, in the button's own text, that it replaces the free scan's answers for criticism and anger
   and leaves ALL-CAPS alone.
3. Add a field for the producer to the stored record, and stop hard-coding `"heuristic"` in the
   export, so "AI" survives a reload. Without this, the label in the Review table is true only
   until the page is reloaded.

Item 3 is a backend change and a stored-shape change; items 1 and 2 are page-only. None of this is
written anywhere as a decision yet — it is my recommendation, not something already agreed.
```

---

## G1-q3-r1

**Variant on disk (not what reached the instance):** G1  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is verified versus what is not

Read-only pass. I ran no tests and wrote no code. Everything below is either something I read in a
file, or something I read in a saved log — I say which, and I mark the places where I am guessing.

## A mismatch I have to state before anything else

The writing rule in the copy of [CLAUDE.md](CLAUDE.md) I was handed is the one that says to use a
word only if it appears in the user's messages, in that file, or in the codebase. The copy of
[CLAUDE.md](CLAUDE.md) sitting on disk right now says something different: a new section headed
`## Vocabulary` telling me to write at a postgraduate reading level. The last commit has neither
section.

So the file that was supposed to steer this answer does not match the file I was steered by. I do
not know which way round the mistake is — either the on-disk file was replaced after my copy was
taken, or my copy is a leftover from an earlier moment. I followed the copy I was given, because
that is what I can actually read. This matters because the name on the file I am writing to says
one thing and the rule I obeyed says another, so whoever reads these answers should not treat this
one as a clean reading of either rule.

## Where the work left off

Three plans live in [docs/plans/](docs/plans/), and they split cleanly into two eras.

**The code era ended on 29 September.** Every phase of
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
is marked Done in its own table. The last three commits that touched code were:

- `da1457a` let the in-memory stores be emptied,
- `d725cc3` added `POST /_dev/reset` plus a test that the `_dev` routes stay out of the deployable
  build, and
- `d81b89e` emptied the backend before each browser test.

**The current era is not about this tool at all.** It is an experiment about which written
instruction in [CLAUDE.md](CLAUDE.md) reduces words a reader has to stop at. Its first attempt is
written up in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md),
and its second attempt is planned in
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).

**That second attempt is running right now, and it is unfinished and uncommitted.** What I observed:

- [CLAUDE.md](CLAUDE.md) is modified against the last commit. Two changes: a one-line wording fix in
  the Workflow section (adding "already-approved"), which reads like the user's own edit, and the
  added `## Vocabulary` section, which is the experiment's own manipulation. The plan's own stopping
  rule says to put the file back and check its hash after each condition. It is not back.
- The v2 plan says it will produce three committed files — the answers verbatim, a table of numbers,
  and the write-up. None of the three exist. [docs/analysis/](docs/analysis/) holds only the first
  attempt's write-up.
- The v2 plan links to a results file that does not exist yet, so that link is dead as written.
- The answers themselves are sitting in this session's scratch directory, outside the repository,
  untracked. Counting the files there: the conditions labelled baseline, A, C and E have their three
  questions answered, G1 is partway through (mine is one of its files), and the two labelled G2 and
  H have not started. So roughly two-thirds done, and none of it captured anywhere durable.

## What is verified

**The Rust tests pass.** I read a saved log at `scratchpad/cargo-test.log` timestamped 03:59 on
30 September: 179 passed, 0 failed, no panics, no compile errors. Counting test markers in the four
crates' `tests/` directories gives 174, which is consistent with that number once the few tests
that live inside the crates are added.

**Caveat on the provenance of that log, which I cannot resolve by reading.** It sits in the
experiment's scratch directory, and the first attempt's write-up records that some experiment
instances ran the test suite themselves. So I can say the suite passed at 03:59; I cannot say it
passed in a run done *for the project* rather than as a side effect of the experiment. The same
applies to the browser suite: `e2e/test-results/.last-run.json` says `"status": "passed"` with an
empty failure list, timestamped 04:14 on 30 September, and that file is deliberately not tracked in
git. Both are real observations of a passing suite; neither has a commit behind it.

**The browser suite covers the six finished phases.** Seventeen tests across
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and [e2e/views.spec.js](e2e/views.spec.js): the
calendar's day rows and a day click, opening a conversation, each of the five analytics views, the
review table's search and filter and paging, a flag the backend set showing as set, the annotated
download carrying flags, an upload with detection not asked for producing no flags at all, the
detection pass reporting progress, the upload reporting bytes sent, the location being written to
the address bar so Back works, an open conversation being addressable, a reload bringing the session
back and saying so, and "Load a different file" stopping it coming back.

**The big deletion happened and is visible in the file.** [timeline.html](timeline.html) is now
3,110 lines and 119,073 bytes. The plan measured it at 66,839 lines and 752,370 bytes before. I
grepped: the embedded word list and the sentiment table are gone, the drift count and its modal are
gone, and the false line telling the user their export "is never uploaded anywhere" is gone. The
plan records this step as checked byte-for-byte identical on screen before and after.

**Detection really did move out of the upload.** I read the doc comment on `process_upload` in
[backend/timeline-api/src/processing.rs](backend/timeline-api/src/processing.rs), which states that
an upload leaves its messages with no flags until detection is asked for, and there is a separate
route in [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)
taking an offset and a limit so the page can walk through the work in batches and move a bar. It has
its own tests at [backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs).

**The one route that must never ship is now guarded by a test.**
[backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
exactly what the deployable branch builds and checks every `_dev` path is absent while the real
routes are present. That matters more since `POST /_dev/reset` arrived, because that route empties
everything.

**Parts of the backend were driven by hand, and this is written down honestly.**
[backend/README.md](backend/README.md) has a section splitting what was run from what was only
read. Run by hand: the local server over real requests, including rejecting no token and a bad
token; the token-checking crate against a real self-signed key pair; the in-memory stores through
their real public methods. Also run: the real cross-compiled binary for the deployment target, and
four requests sent at it through a local stand-in for the service's own request-handing mechanism.
That last session is not captured as a repeatable test, and the file says so.

## What is not verified

**The real S3 and DynamoDB code has never talked to anything.**
[backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) and the files under
[backend/timeline-storage/src/dynamo/](backend/timeline-storage/src/dynamo/) compile and have their
request-building tested, but no send has ever reached a real service or a local stand-in. Only
in-memory tests exist in [backend/timeline-storage/tests/](backend/timeline-storage/tests/). This is
open as C10 at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926),
with no work started.

**Nothing is deployed.** [infra/template.yaml](infra/template.yaml) has never been checked or
deployed. There is no real login — `POST /_dev/login` stands in for one. Nothing persists: every
store is a map in the running process, so restarting deletes every upload, conversation and flag.

**Everything past the local dev harness is unbuilt.** No classification through Amazon Bedrock, no
payment, no production hardening. I confirmed by listing files: there is no `classify.rs`, no
`bedrock.rs`, no payments module anywhere under [backend/](backend/).

**A plan entry is marked resolved and I cannot find what it claims.** C12 at
[docs/plans/2026-09-28-frontend-quality-of-life.md:444](docs/plans/2026-09-28-frontend-quality-of-life.md#L444)
is tagged RESOLVED and says one command now runs every suite — the Rust workspace, the browser
tests and the launcher test — and pins the version of Node the browser tests need; it also says a
check for swallowed exceptions covering both the Rust and the JavaScript was in scope. I looked:
[scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh` and `test-dev-up.sh`;
[.vscode/tasks.json](.vscode/tasks.json) has no such task; [backend/README.md](backend/README.md)
still tells you to run `cargo test --workspace` on its own; and no file matching that check exists
anywhere. So either it landed somewhere I did not find, or the tag is wrong. I am reporting this as
"I could not find it", not as "it does not exist" — but I searched the obvious places.

**One commit claims more than it did.** `d81b89e` is titled "Reset the backend before each e2e
test". Its diff touches only [e2e/views.spec.js](e2e/views.spec.js).
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) still has only a run-once setup and never calls
the reset route, so one of the two files got the clean slate.

**The first known defect is still there, and I read the code rather than inferring it.**
`list_for_user` at
[backend/timeline-storage/src/memory/conversations.rs:36](backend/timeline-storage/src/memory/conversations.rs#L36)
still walks a map and collects without sorting. Rust seeds that map's hashing differently per
process, so the conversation list and its colours still reshuffle between restarts. The plan records
this at
[docs/plans/2026-09-28-frontend-quality-of-life.md:471](docs/plans/2026-09-28-frontend-quality-of-life.md#L471)
as observed directly — three runs of the same page, three different orders.

**The second known defect was worked around, not explained.** An intermittent failure of the page
to reach the backend while the backend was demonstrably answering. Waiting for the backend before
driving the browser made it stop happening. Nobody found the cause. Recorded at
[docs/plans/2026-09-28-frontend-quality-of-life.md:479](docs/plans/2026-09-28-frontend-quality-of-life.md#L479).

**[backend/README.md](backend/README.md) contradicts the code in three places.** Its "What's not
built yet" section at line 216 says the upload processing is not built, that the export route is not
built, and that [timeline.html](timeline.html) is "still 100% unmodified" with nothing in the
browser calling the backend. All three are now false — there is a `process_upload`, there is an
[export route](backend/timeline-api/src/routes/export.rs), and the page has been wired since commit
`b9122ce`. The same file's test count at
[backend/README.md:66](backend/README.md#L66) says 163, where the saved log shows 179.

**There is no continuous-integration setup of any kind.** No `.github` directory. Both suites run
only when someone remembers.

**The line-count ceiling check only sees part of the repository.**
[backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) passes with
an empty exemption list, but it reads only `.rs` files under [backend/](backend/), so
[timeline.html](timeline.html) is outside its reach. Now that the page is 3,110 lines rather than
66,839, extending it is finally meaningful.

**Some tests reach into private code and cannot yet be replaced.**
[backend/timeline-storage/src/dynamo/message_flags_table.rs](backend/timeline-storage/src/dynamo/message_flags_table.rs)
still carries an inline test block. The project's own rule says such a test is scaffolding, to be
removed once a test through the public interface proves the same thing. That cannot happen while the
DynamoDB code has no way to be driven from outside, which ties this to C10 above.

**Two more plan entries are open in the migration plan.** C6 at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:875](docs/plans/2026-09-09-rust-aws-backend-migration.md#L875)
needs the user's go-ahead before capturing one real sample exchange with the classification service.
C8 at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:890](docs/plans/2026-09-09-rust-aws-backend-migration.md#L890)
is a counted mismatch between an old statistic in
[timeline-project-decisions.md](timeline-project-decisions.md) and a fresh measurement; the
arithmetic lines up with one explanation, but the original file no longer exists so it cannot be
closed, and the write-up correctly calls it a strongly supported guess rather than a fact.

**The "Classify with AI" button is still in the page and still untouched.** I grepped: the button
and its handler are at [timeline.html](timeline.html) lines 844 and 1809. It still calls the
Anthropic service straight from the browser, which only works inside a rendered Claude artifact. No
test of any kind touches it. The coverage audit already suggested deciding whether to delete it
rather than test it, and how the two kinds of emotion detection are named for users is tabled at
[docs/plans/2026-09-28-frontend-quality-of-life.md:487](docs/plans/2026-09-28-frontend-quality-of-life.md#L487)
for the user to design.

**One smaller thing left open on purpose.** C1 at
[docs/plans/2026-09-28-frontend-quality-of-life.md:364](docs/plans/2026-09-28-frontend-quality-of-life.md#L364):
the page's own parsing step still cannot show real progress, because it holds the browser's single
working thread for its whole duration. Its trigger for revisiting is whether it still looks frozen
now that the page is small — and that measurement has not been taken.

## The shortest version

Six phases of page-and-server work are finished and covered by tests that were passing a few hours
ago, though the passing runs are logs in a scratch directory rather than anything a commit points
at. Everything that needs a real cloud account — durable storage, real login, classification,
payment, deployment — is unbuilt and unverified, and that gap is written down rather than hidden.
Four things I would raise with the user before calling the state clean: a plan entry marked resolved
whose work I cannot find, a stale "what's not built" section that now contradicts the code, the
conversation-order defect that is still live, and the fact that
[CLAUDE.md](CLAUDE.md) is currently carrying an experiment's edit uncommitted.
```

---

## G1-q3-r2

**Variant on disk (not what reached the instance):** G1  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work stopped, and what is actually checked

Two separate lines of work. One is finished and committed. The other is running right now, and it
has a fault in it that I found while reading, which I put at the end because it needs a decision
from you rather than from me.

## Line one: the six-part pass over the page and the dev setup — finished

[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
lists six parts and marks all six **Done** in its own table at
[lines 12-17](docs/plans/2026-09-28-frontend-quality-of-life.md#L12-L17). Reading the code, that
matches what is on disk:

| Part | What I read that shows it landed |
|---|---|
| Launcher and the editor buttons | [scripts/dev-up.sh](scripts/dev-up.sh), [scripts/dev-down.sh](scripts/dev-down.sh), [scripts/test-dev-up.sh](scripts/test-dev-up.sh), five entries in [.vscode/tasks.json](.vscode/tasks.json) |
| Browser tests for the calendar and the five number views | [e2e/views.spec.js](e2e/views.spec.js), 13 tests |
| Deleting the page's own flag-finding code and its two word lists | [timeline.html](timeline.html) is now 3,110 lines / 119,073 bytes, down from the 66,839 lines / 752,370 bytes the plan measured. `DICTIONARY_WORDS_RAW` and `AFINN` return zero matches |
| Flag-finding only when asked, with something to watch | `runDetectionPass` at [timeline.html:1303](timeline.html#L1303) calls `POST /detect`; the box that turns it on is read at [timeline.html:1334](timeline.html#L1334); the route is served at [backend/timeline-api/src/app.rs](backend/timeline-api/src/app.rs) and has its own test file [backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs) |
| Upload progress | covered by a test named "the upload reports byte progress before the server-side wait" at [e2e/views.spec.js:367](e2e/views.spec.js#L367) |
| Back button and coming back to where you were | `hashchange` listener at [timeline.html:2349](timeline.html#L2349), read/write at [timeline.html:2324-2328](timeline.html#L2324-L2328) |

The last four commits on this line are `d725cc3` (a `POST /_dev/reset` route plus a test that the
`_dev` routes are absent from the deployable build), `da1457a`, `d81b89e` (emptying the backend
before each browser test) and `7cb86d9`.

## Line two: the written-instruction experiment — running, roughly two thirds through

This is where the newest commits are. `af47dae`, `54c9e19`, `57c8d3b` and `966655d` are all about
it.

- The first attempt is written up in
  [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md).
  It reached two runs per condition on three conditions out of a planned ten, and its own
  [Findings](docs/analysis/2026-09-30-vocabulary-instruction-results.md#L187) say the way it counted
  words cannot tell `hash-keyed` from `whose`, so the table in it does not carry the meaning it
  appears to.
- The second attempt is planned in
  [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  — seven conditions, ten runs each, three of your own questions.
- **It is part way through as I write this.** Counting the saved answers: baseline has 10, A has 9,
  C has 9, E has 9, and the fifth condition is being collected now. Two conditions have not started.
  Timings and token counts for the 37 finished runs are in `durations.csv` beside those answers.
- None of the three files the plan promises
  ([lines 125-142](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L125-L142)) exist
  yet. No results, no table of numbers, no collected answers in
  [docs/analysis/](docs/analysis/). Everything so far lives outside the repository and is not
  committed.
- Two files are uncommitted: [CLAUDE.md](CLAUDE.md), which currently holds a swapped-in section
  rather than your own text, and the second plan.

### The count does not match the plan

The plan says ten runs per condition. Three of the four finished conditions have nine. That is not
something I can explain from reading — I can see nine files where ten were specified, and I do not
know whether a launch was dropped, a run failed without writing, or nine was a deliberate change.
Worth settling before the numbers are written up, because a condition with nine and a condition
with ten are not directly comparable on any total.

## Checked, in the sense that something would fail if it broke

- **The Rust side.** 168 test functions across 24 files under
  [backend/](backend/), including [backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs)
  for the new route and [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs)
  for the one property where being wrong means shipping something that mints tokens without asking
  for a password. I did not run them — you asked me not to — so this is a count of tests that exist,
  not a statement that they pass.
- **The browser side.** 16 tests across [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) (3) and
  [e2e/views.spec.js](e2e/views.spec.js) (13), driving the real backend and the real file. These
  cover the calendar, all five number views, opening a conversation, the review controls, the
  download, no flags unless asked, progress while flag-finding runs, upload progress, the hash and
  the Back button, coming back after a reload, and picking a different file.
- **The line-count ceiling**, at
  [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs), with an
  empty list of exceptions.

## Not checked, and why

1. **The two known faults from the last pass are both still there**, listed at
   [lines 466-485](docs/plans/2026-09-28-frontend-quality-of-life.md#L466-L485).
   - The order conversations come back in is still unfixed. `list_for_user` at
     [backend/timeline-storage/src/memory/conversations.rs:36-48](backend/timeline-storage/src/memory/conversations.rs#L36-L48)
     still walks the map and collects with no sort. I read the code; nothing sorts.
   - The occasional "Failed to fetch" from the page was worked around, not explained. The plan
     says so itself.
2. **Emptying the backend before each test only happens in one of the two browser test files.**
   Commit `d81b89e` is titled "Reset the backend before each e2e test" and its own list of changed
   files shows it touched [e2e/views.spec.js](e2e/views.spec.js) alone.
   `POST /_dev/reset` appears nowhere in [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js). So
   three of the sixteen tests still start from whatever the previous run left behind. Whether that
   matters depends on the order they run in, which I did not trace.
3. **"One command runs every test" is claimed but I cannot find it.** The resolution written at
   [line 444](docs/plans/2026-09-28-frontend-quality-of-life.md#L444) says one command now runs the
   Rust tests, the browser tests and the launcher test, and pins the Node version. I searched
   [scripts/](scripts/), both `package.json` files, [.vscode/tasks.json](.vscode/tasks.json) and
   [README.md](README.md) and found no such command. [README.md:84](README.md#L84) still tells you
   to run the browser tests by hand. Either it was never written, or it is somewhere I did not
   look — but the plan records it as settled, which on what I can see it is not.
4. **The check for quietly-swallowed errors, also recorded in that same resolution, does not
   exist.** [CLAUDE.md](CLAUDE.md) refers to `tests/test_no_unhandled_exceptions.py`. There is no
   such file and no Python at all in this repository. The resolution says it has to be written; it
   has not been.
5. **The line-count ceiling still cannot see the page.**
   [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) collects
   only `.rs` files under [backend/](backend/). [timeline.html](timeline.html) is now small enough
   for such a rule to mean something, which was the stated trigger for adding one.
6. **The 77.57% figure in the plan is stale.** It was measured before the large deletion and before
   the new route. I have no current number and did not produce one.
7. **Nothing has ever run against real S3 or DynamoDB.** Still open at
   [line 926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926) of the migration plan. That
   plan's later stages — the paid classification pass and the charge — are unstarted.
8. **`classifyWithAI` is still in the page and still untested.** The button is at
   [timeline.html:844](timeline.html#L844) and the function at
   [timeline.html:1809](timeline.html#L1809). Its own comment at
   [timeline.html:1598](timeline.html#L1598) calls it artifact-only and slated for replacement. The
   earlier audit suggested deciding whether to delete it rather than test it; that decision has not
   been made.
9. **Two open items from the last pass, both waiting on you**, not on code: whether the
   client-side parse can show real progress ([line 364](docs/plans/2026-09-28-frontend-quality-of-life.md#L364))
   and how restoring a session on startup should announce itself
   ([line 402](docs/plans/2026-09-28-frontend-quality-of-life.md#L402)). The second was answered in
   the table at the top of the plan (announced) but the entry was never closed.
10. **How the two tiers of flag-finding are named for users is still tabled**, at
    [line 487](docs/plans/2026-09-28-frontend-quality-of-life.md#L487). You said you would design
    it.

## The thing I would stop for: this run was given the wrong instructions

I checked which instruction file I am actually running under, because the answer affects whether
this run counts. It does not match.

- [CLAUDE.md](CLAUDE.md) on disk right now is byte-for-byte the fifth condition's file. Both are
  31,870 bytes and both hash to `9ecbd99d39d190b4d205ddd2e5e2443b`. Its writing section is a single
  sentence about writing at a postgraduate reading level.
- The instructions I was handed are a **different file**: 33,638 bytes, hashing to
  `3d5a899f835aeb58e0563c2f39d615cd`, saved beside the experiment as `CLAUDE.md.ORIGINAL`. That is
  your own uncommitted working file. Its writing section is the long one headed "Only use words that
  are already in the project", the one with the two-lookups rule and the worked list ending in
  *provenance* and *coercion*.
- That text appears in no condition file. The nearest one, the third condition's, is worded
  differently and carries a paragraph exempting ordinary English which the text I was given does not
  have.
- The saved hash of your original, in `original.sha256`, does not match what is on disk now, which
  is expected mid-experiment — the swapped-in file is there on purpose. What is not expected is that
  the swap did not reach me.

So this run is labelled as the fifth condition and is answering under your original file instead. I
cannot tell from here whether the same is true of the other runs in this condition, or of the four
conditions already collected — that would need checking each run's own record, which I do not have
access to. If it is general, the finished answers do not measure what the plan says they measure.

Per [CLAUDE.md](CLAUDE.md)'s own rule about stopping when a wiring fault turns up mid-run, I am not
going to guess and I am not going to write the answer as though the labelling were sound. The
decision is yours: check the other runs and redo the affected ones, keep them with the mismatch
recorded in the write-up, or stop and fix how the file reaches a run before collecting any more. I
have left this answer in the place the label says, with the mismatch stated here rather than in a
footnote, so it cannot be counted as a clean fifth-condition run by accident.
```

---

## G1-q3-r3

**Variant on disk (not what reached the instance):** G1  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work stopped, and which claims rest on a run rather than on reading

Everything below comes from reading files and git history. I ran no tests, per the request, so the
only evidence here that anything works while running is one recorded Playwright result, noted as
such.

## Where it stopped

The last commits that touched the product are, newest first:

- `d81b89e` — [e2e/views.spec.js](e2e/views.spec.js) empties the backend before each test.
- `d725cc3` — adds [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs)
  and [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs).
- `da1457a` — the in-memory stores gain a way to be emptied.
- `721d940` — marks all six phases of
  [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
  done.

Since then, four commits concern how instructions are written, not the product: the experiment in
[docs/plans/2026-09-30-vocabulary-instruction-experiment.md](docs/plans/2026-09-30-vocabulary-instruction-experiment.md),
its findings in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md),
a correction of two sentences there, and a second design in
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md).

**That second design has not been carried out.** It names three files it will produce; the only file
in [docs/analysis/](docs/analysis/) is the first attempt's findings. So the plan is committed and the
runs behind it are not there.

Two files are changed and not committed: [CLAUDE.md](CLAUDE.md) (a vocabulary section added, and one
line of the workflow text reworded) and the second experiment design. The design itself warns that
[CLAUDE.md](CLAUDE.md) carries the user's own unsaved work, which makes this the riskiest thing in
the tree right now.

## What I can show from reading the code

- The two large word lists are gone from [timeline.html](timeline.html), which is now 3,110 lines and
  119,073 bytes — the size the deletion predicted.
- Detection happens only when asked. `runDetectionPass` at
  [timeline.html:1303](timeline.html#L1303) sends batches to `POST /detect` and is reached only when
  the load-screen box is ticked ([timeline.html:1334](timeline.html#L1334)).
  [backend/timeline-api/tests/detect.rs](backend/timeline-api/tests/detect.rs) has tests named for an
  upload having no flags until detection is asked for, the flags being the real computed ones rather
  than a stand-in, assistant turns never getting one, and paging covering every conversation once.
- The browser location is written and read back
  ([timeline.html:2324-2349](timeline.html#L2324-L2349)), and the one request carrying the file body
  uses `XMLHttpRequest` so its bytes can be reported ([timeline.html:1176](timeline.html#L1176)).
- [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  what the deployable branch builds and asserts the `_dev` paths are missing while the real ones are
  present but refuse an unauthenticated caller.
- [e2e/views.spec.js](e2e/views.spec.js) holds 14 named tests across the calendar, an opened
  conversation, all five analytics views, the review controls, a flag the backend produced showing up
  on screen, the annotated download, detection progress, upload progress, back and forward, and
  restoring on reload.

## The only running evidence, and how thin it is

[e2e/test-results/.last-run.json](e2e/test-results/.last-run.json) reads `"status": "passed"` with no
failed tests, written at 04:14 on 30 September — after the last product commit. It does not record
which files ran or how many tests, it is excluded from version control by
[.gitignore](.gitignore), and nothing else on disk records a run. So "the browser suite passed" is
supportable; "these 14 tests passed" is not, from what is here.

## Not verified, and each one for a different reason

1. **The stored-in-Amazon code has never run against anything.**
   [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) says so in its own opening
   comment: no coverage at all, never pointed at real or local S3. The DynamoDB files say their key
   and expression building is tested and that they have never spoken to a real or local table. The
   matching entry, C10, in
   [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
   is open and calls this unstarted.
2. **The quoted Rust numbers are stale.** "173 tests, 77.57% of lines" in the quality-of-life plan was
   measured before the last four phases landed. It does not describe the tree as it stands.
3. **A defect the plan lists as known is still in the code.** `list_for_user` at
   [backend/timeline-storage/src/memory/conversations.rs:36](backend/timeline-storage/src/memory/conversations.rs#L36)
   walks a `HashMap` and collects with no ordering step, which is the cause of conversations coming
   back in a different order after each restart. The fix named in the plan — order by something
   stable — is not applied.
4. **The page occasionally failing to reach the backend was worked around, not explained.** The plan
   says so plainly and calls it not root-caused.
5. **Emptying the backend between tests reached only one of the two test files.**
   [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no such step and no per-test setup at all,
   and it starts its own server without first checking whether one is already answering on the same
   port, which the other file does check.
6. **Two things the quality-of-life plan marks resolved under C12 are absent from disk.** There is no
   single command that runs the Rust tests, the browser tests and the launcher test together —
   [scripts/](scripts/) holds only the two start/stop scripts, a port helper, and the launcher's own
   test — and there is no check for exceptions being caught and dropped in silence, in either
   language. [CLAUDE.md](CLAUDE.md) names `tests/test_no_unhandled_exceptions.py`; there is no such
   directory and no Python in this repository. This one needs the user's decision: either the entry
   goes back to open, or the two pieces get written.
7. **The line-count guard cannot see the page.**
   [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) walks `.rs`
   files under [backend/](backend/) only. The plan already records this as left over.
8. **Nothing runs on its own.** There is no `.github` directory, so every suite runs only when
   someone remembers to run it.
9. **The paid classification step and the payment step do not exist.** No reference to either Amazon's
   model service or the payment provider appears anywhere under [backend/](backend/) or
   [infra/](infra/). [infra/template.yaml](infra/template.yaml) exists; nothing on disk shows it has
   ever been deployed.
10. **Two smaller leftovers, both deliberate and documented.** `classifyWithAI`
    ([timeline.html:1809](timeline.html#L1809)) is still in the page, untested, and marked for
    replacement; the artifact storage call at [timeline.html:1611](timeline.html#L1611) is kept on
    purpose for that feature's checkpoint. How the two tiers of detection are named for users is
    tabled at the user's direction.

## The shortest way to close the gap

Run the three suites and record what passed, since the current record cannot say. Then decide item 6
above, because a plan that marks work resolved while the work is missing will keep being read as
evidence that it is done.
```

---

## baseline-q1-r1

**Variant on disk (not what reached the instance):** baseline  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and most of it is already here — the browser tests under [e2e/](e2e/) do it in one
file and not the other, and the Rust tests solve the same problem a different way.

## What exists today

Two of the standard answers are already in the repo:

1. **Reset shared state in a per-test setup hook.** [e2e/views.spec.js:128](e2e/views.spec.js#L128)
   runs `resetBackend()` before every test, and that helper
   ([e2e/views.spec.js:50](e2e/views.spec.js#L50)) sends `POST /_dev/reset`. The server side is
   [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs),
   which empties only the stores that implement the `Resettable` trait at
   [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24)
   — the in-memory fakes, and nothing else, so it cannot reach a real S3 bucket or DynamoDB table.

2. **Build fresh state per test instead of resetting shared state.** Each Rust integration test
   assembles its own router over its own stores, so construction *is* the reset. They pass an empty
   list of resettable stores deliberately — [backend/timeline-api/tests/detect.rs:66](backend/timeline-api/tests/detect.rs#L66),
   [export.rs:62](backend/timeline-api/tests/export.rs#L62), [dev_routes.rs:62](backend/timeline-api/tests/dev_routes.rs#L62).
   This is the better shape whenever it's affordable, because there is no ordering to get wrong.
   The browser tests can't use it: they drive one long-lived `cargo run -p timeline-api` process.

Browser-side state is handled by the test framework rather than by anything we wrote: Playwright
gives every test a fresh browser context, and [e2e/playwright.config.js](e2e/playwright.config.js)
sets no saved-storage option, so the `timeline_api_base` and `timeline_dev_sub` values that
[timeline.html:1049](timeline.html#L1049) and [timeline.html:1075](timeline.html#L1075) write start
out empty in each test.

## The gap I found

[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has **no** per-test reset — it has only
`beforeAll`/`afterAll` ([lines 53](e2e/upload-flow.spec.js#L53) and [76](e2e/upload-flow.spec.js#L76)).
It currently starts from an empty server anyway, but only because of two accidents: `workers: 1`
([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)) serializes the files, and
`upload-flow` sorts before `views`, so it is the file that spawns the server and runs first. Rename
a file, add a third one, or let the files run in parallel and that stops being true. I have not run
the suite in a different order to watch it fail — this is read from the two files plus the config,
not observed.

Its three tests do carry real cross-test state: the flag-persistence test at
[e2e/upload-flow.spec.js:130](e2e/upload-flow.spec.js#L130) asserts `OVERRIDES` is non-empty after a
re-upload, which would also pass on a confirmed flag left behind by an earlier run against a server
that was already up.

## What I'd suggest

- **One line, no new machinery:** add the same `beforeEach` reset to
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js). Closes the gap; leaves the duplication.
- **The tidier version:** move `resetBackend` and the reset hook into one shared module that
  re-exports a `test` with the hook already attached (Playwright's `test.extend(...)` with
  `{ auto: true }` runs a setup step for every test in every file that imports it). New spec files
  then get the clean slate without remembering to ask, which is the property that just failed.
- **Separately, the server half:** both spec files hand-roll `spawn` + a wait-for-port loop, and
  [e2e/views.spec.js:90](e2e/views.spec.js#L90) carries a guard against binding port 3000 twice.
  Playwright's config has a built-in `webServer` option that starts a command, waits for a URL, and
  shuts it down — it would delete all of that duplicated code. Worth doing in the same pass, but
  it's a bigger change than the reset hook and I'd rather price it separately.

Want me to write this up in [docs/plans/](docs/plans/) before touching code? My read is that the
first item is small enough to just do, but the shared-setup module and the `webServer` switch
together are a plan-sized change, and I shouldn't start either without your approval.
```

---

## baseline-q1-r2

**Variant on disk (not what reached the instance):** baseline  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — Playwright has two built-in mechanisms for exactly this, and we're currently
hand-rolling both of them (one of them only in half the files).

## What's there now

- The backend already has the reset hook: the `Resettable` trait in
  [backend/timeline-storage/src/memory/resettable.rs](backend/timeline-storage/src/memory/resettable.rs)
  and `POST /_dev/reset` in
  [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs).
  That part is in good shape, and the route is structurally absent from the Lambda build.
- [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130) calls it from a `test.beforeEach`.
- [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) **never calls it at all** — it has a
  `beforeAll` at [line 53](e2e/upload-flow.spec.js#L53) that starts the server and nothing that
  resets between its three tests. So "reset before each browser test" is currently true of one
  spec file out of two. With `workers: 1` ([e2e/playwright.config.js:14](e2e/playwright.config.js#L14))
  the files run in one process in filename order, so `upload-flow` runs first against a
  freshly-spawned server and happens to start clean — that's my reading of the ordering rule, not
  something I ran to confirm. Its three tests still share whatever each other leaves behind.

## The standard way, part 1: an automatic setup step shared across files

Playwright's own mechanism for "this must happen before every test, in every file" is to extend
the imported `test` object once, in a shared module, with an automatic setup step
(`{ auto: true }`), then have every spec import that instead of `@playwright/test`:

```js
// e2e/timeline-test.js
const base = require('@playwright/test');
exports.test = base.test.extend({
  cleanBackend: [async ({}, use) => {
    const res = await fetch('http://127.0.0.1:3000/_dev/reset', { method: 'POST' });
    if (!res.ok) throw new Error(`could not reset the backend: ${res.status}`);
    await use();
  }, { auto: true }],
});
exports.expect = base.expect;
```

`{ auto: true }` is the part that matters here: the spec doesn't have to ask for it, so a new spec
file can't forget it the way [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) did. A
per-file `beforeEach` is a convention each author has to remember; this is a default they'd have to
actively opt out of. The same module is the natural home for the other three things both spec files
currently keep their own copies of — `API_BASE`, the path to the sample export file checked into
the repo, and the load-then-wait-for-`#mainContent` helper.

## The standard way, part 2: let the config own the server

Both spec files duplicate a `spawn('cargo run -p timeline-api')`, a `waitForPort` poll loop, and a
kill in `afterAll` — including the "is one already listening?" check that
[e2e/views.spec.js:90-100](e2e/views.spec.js#L90-L100) has and
[e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53) doesn't. Playwright's config has a
`webServer` block for this, with a `reuseExistingServer` option that is precisely the check
`views.spec.js` wrote by hand:

```js
webServer: {
  command: 'cargo run -p timeline-api',
  cwd: '../backend',
  url: 'http://127.0.0.1:3000/conversations',
  reuseExistingServer: true,
  timeout: 90_000,
},
```

That deletes roughly 40 lines from each spec file and removes the process-group kill juggling in
[e2e/views.spec.js:132-141](e2e/views.spec.js#L132-L141). One thing to check before committing to
it: the `PATH` additions for `cargo` and `zig` that both `beforeAll` blocks assemble — `webServer`
takes an `env`, so this looks like it carries over, but I haven't run it.

## Not affected

The Rust integration tests don't need any of this. [backend/timeline-api/tests/detect.rs:40-68](backend/timeline-api/tests/detect.rs#L40-L68)
constructs fresh `InMemory*` stores per test, so each one starts empty by construction. Explicit
reset is only needed by the browser tests, because they're the only ones sharing a long-lived
server process.

## What I'd want from you

This is a change to committed tests plus the config, so per our workflow I'd write it up in
`docs/plans/` first rather than start editing. Two decisions I'd want your call on:

1. Do both parts, or just part 1? Part 1 fixes the actual gap (upload-flow never resets). Part 2 is
   a cleanup that touches the server lifecycle, which is the piece most likely to cost us a
   debugging session if `webServer` and the `PATH` business don't get along.
2. Making upload-flow's three tests reset between themselves may break them — they may be relying
   on the earlier test's upload. If so the fix is each test loading the sample file itself, which is
   a real edit to committed tests, and I'd want your approval on the shape before I touch them.
```

---

## baseline-q1-r3

**Variant on disk (not what reached the instance):** baseline  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes — and you already built half of it. There is a standard mechanism in Playwright for
"every test must start from a known state," and it is not `beforeEach`.

## What exists now

The reset itself is done and wired:

- `POST /_dev/reset` at [backend/timeline-api/src/routes/dev_reset.rs:24](backend/timeline-api/src/routes/dev_reset.rs#L24), mounted at [backend/timeline-api/src/app.rs:55](backend/timeline-api/src/app.rs#L55).
- It walks a list of stores that can be emptied — the `Resettable` trait at [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24).
- The list is built at [backend/timeline-api/src/main.rs:92](backend/timeline-api/src/main.rs#L92) and holds four stores. I grepped `InMemory` in [backend/timeline-api/src/main.rs](backend/timeline-api/src/main.rs): the binary constructs exactly those four, and all four are in the list. So the reset's coverage of server-side state is complete as of now — nothing is silently left behind.
- [e2e/views.spec.js:128](e2e/views.spec.js#L128) calls it in a `test.beforeEach`.

## The gap I found

[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets. It has a `beforeAll`
([line 53](e2e/upload-flow.spec.js#L53)) and an `afterAll` ([line 76](e2e/upload-flow.spec.js#L76)) and no
`beforeEach` at all. It currently gets away with it because `workers: 1`
([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)) plus alphabetical file order means
`upload-flow` runs first, against a server it just started. That is luck, not isolation — rename a
file and it breaks.

This is exactly the failure mode `beforeEach` has: it is per-file opt-in, so a second file can
forget, and one already did.

Separately, the two files' server startup has diverged and I think the divergence is load-bearing by
accident. [e2e/views.spec.js:90](e2e/views.spec.js#L90) checks for an already-listening server and
reuses it, spawns `detached` and kills the whole process group. [e2e/upload-flow.spec.js:53](e2e/upload-flow.spec.js#L53)
spawns unconditionally and kills only the `cargo` process, not the server it spawned — so the server
probably survives `upload-flow`'s `afterAll` still holding port 3000, and that surviving process is
what `views.spec.js` then "reuses." I have not traced this with `ps` to confirm it; it is what I read
in the two files, not something I watched happen.

## The standard way

Two Playwright features, both built in:

**1. An automatic fixture, for the reset.** A shared `e2e/fixtures.js` extends the base `test` with a
fixture marked `{ auto: true }` — it runs for every test whether or not the test mentions it. Spec
files then `require('./fixtures')` instead of `@playwright/test`. Shape:

    const base = require('@playwright/test');
    exports.test = base.test.extend({
      freshBackend: [async ({}, use) => { await resetBackend(); await use(); }, { auto: true }],
    });

The difference from `beforeEach` is the whole point: a new spec file cannot forget, because forgetting
would mean importing from somewhere else. It is also where `resetBackend`, `loadFixture`, `uniqueSub`
and the shared constants belong, instead of being copied into each file.

**2. `webServer` in [e2e/playwright.config.js](e2e/playwright.config.js), for the server.** Playwright
starts one server for the whole run, polls a URL until it answers, and tears it down at the end;
`reuseExistingServer` covers the "one is already up" case. That deletes both `beforeAll` blocks, both
copies of `waitForPort`, the `startedServerHere` flag, and the hand-rolled process-group kill — and
with it the divergence above.

One thing worth naming so the reset's scope stays honest: browser-side state is already neutral per
test, because Playwright gives each test a fresh browser context, so `localStorage` and
`sessionStorage` do not carry over. That matters here because `views.spec.js` has tests about session
restore across reloads. The reset is for server state only; do not widen it to "clear the browser
too" thinking that is missing.

The Rust tests do not have this problem and need no change — each one builds its own app and its own
in-memory stores, so they are isolated by construction.

## On namespacing instead

For the record, the alternative — give each test its own user id and never reset — is already argued
against in the code, at [e2e/views.spec.js:44](e2e/views.spec.js#L44) and in the trait's own doc
comment. A distinct user id avoids collisions but leaves every earlier test's rows sitting in the
store where anything not keyed by user still finds them. I agree with the conclusion already recorded
there; I am not reopening it.

## What I'd need from you

This is a code change across [e2e/](e2e/), so per the workflow it wants a plan first. Say the word and
I'll write `docs/plans/2026-09-30-e2e-test-isolation.md` covering the shared fixture file, the
`webServer` switch, and what happens to the two `beforeAll`/`afterAll` blocks — then you review it
before I touch any code.
```

---

## baseline-q1-r4

**Variant on disk (not what reached the instance):** baseline  
**Question:** It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

```
Yes, and you've already built it — for one of the two browser-test files. Everything below is
read in the source; I have not run the suite this session, so nothing here is a measurement.

## What already exists

The reset-explicitly approach is the one this repo settled on, and your sentence is nearly the
comment that's already in the code:

- `POST /_dev/reset` empties every in-memory store — [backend/timeline-api/src/routes/dev_reset.rs](backend/timeline-api/src/routes/dev_reset.rs),
  registered at [backend/timeline-api/src/app.rs:55](backend/timeline-api/src/app.rs#L55), and only in
  the `_dev` router, so it is absent from the Lambda build.
- The capability behind it is the `Resettable` trait at
  [backend/timeline-storage/src/memory/resettable.rs:24](backend/timeline-storage/src/memory/resettable.rs#L24),
  implemented by all four in-memory stores (object store, conversation summaries, message flags,
  upload outcomes) and by nothing else — so it cannot reach a real S3 bucket or DynamoDB table.
- All four are wired into the list the route walks:
  [backend/timeline-api/src/main.rs:92-97](backend/timeline-api/src/main.rs#L92-L97). I checked this
  specifically, because a reset route that empties three of four stores is worse than none.
- The per-test call is a `beforeEach` hook at
  [e2e/views.spec.js:128-130](e2e/views.spec.js#L128-L130), with the reasoning written out at
  [e2e/views.spec.js:44-49](e2e/views.spec.js#L44-L49) — including the point that a distinct user id
  per test avoids collisions but is not a clean slate.

## The standard mechanism for spreading it across files

Playwright's own answer to "every test file needs this setup" is what its docs call a fixture — a
named setup value the framework builds and tears down around each test, a different sense of the
word from this repo's `FIXTURE` constant (the sample export file). Two kinds are relevant:

- `test.extend({ cleanBackend: [async ({}, use) => { await resetBackend(); await use(); }, { auto: true }] })`
  — `auto: true` means it runs for every test in any file that imports this `test`, without each
  file remembering to write a hook.
- The same call with `{ scope: 'worker', auto: true }` for starting and stopping `timeline-api`
  once per worker process.

Put both in one file (say `e2e/timeline-server.js`), export `test` and `expect` from it, and have
each spec import from there instead of `@playwright/test`. That is also the fix for the ~60 lines of
`spawn` / `waitForPort` / kill boilerplate currently duplicated in
[e2e/upload-flow.spec.js:53-78](e2e/upload-flow.spec.js#L53-L78) and
[e2e/views.spec.js:90-140](e2e/views.spec.js#L90-L140), in two versions that do not agree with each
other.

The config-level alternative, `globalSetup` / `globalTeardown` in
[e2e/playwright.config.js](e2e/playwright.config.js), runs once per whole run. That suits starting
the server but cannot do a per-test reset, so it is the wrong layer for the reset itself.

## Two gaps I noticed while reading

1. **[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) never resets.** It has `beforeAll` at line
   53 and `afterAll` at line 76, no `beforeEach`, and it never fills `#devLoginSub`, so its three
   tests share both one user id and whatever the store has accumulated. Its "confirming a flag
   persists through a reload" test reads `OVERRIDES` after a re-upload — that assertion passes on
   any non-empty override set, so an override left by an earlier test would satisfy it too.

2. **Its teardown can orphan the server.** It spawns without `detached` and calls
   `serverProcess.kill('SIGTERM')`, which signals the `cargo` wrapper rather than the process group,
   so the real `timeline-api` can outlive the run holding port 3000.
   [e2e/views.spec.js](e2e/views.spec.js) spawns detached and signals `-pid` precisely to avoid this,
   and it also reuses an already-listening server — which means, with `workers: 1`
   ([e2e/playwright.config.js:14](e2e/playwright.config.js#L14)) and `upload-flow` sorting first, the
   second file may be running against the first file's orphan. Shared state outside the stores, in
   other words, and the kind a reset route cannot clean up.

Browser-side state looks fine without extra work: Playwright builds a fresh browser context per
test, so the `localStorage` keys `timeline_api_base` and `timeline_dev_sub`
([timeline.html:1049](timeline.html#L1049), [timeline.html:1075](timeline.html#L1075),
[timeline.html:1264](timeline.html#L1264)) start empty each time. The Rust tests need none of this
either — each builds its own stores, and
[backend/timeline-api/tests/dev_routes.rs:58-62](backend/timeline-api/tests/dev_routes.rs#L58-L62)
deliberately passes an empty reset list so a later test that resets without wiring fails loudly.

## What I'd want your decision on

This is a change to the browser tests plus a possible shared setup file, so per the workflow I'd
write it up in `docs/plans/` first rather than start editing. Say the word and I'll draft it. Worth
deciding up front: whether the shared setup file is worth it now, or whether you'd rather I just add
the missing `beforeEach` and fix the teardown in
[e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) — a much smaller change that closes gap 1 and
gap 2 without touching how the other file is organized.
```

---

## baseline-q2-r1

**Variant on disk (not what reached the instance):** baseline  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Short answer: it doesn't any more, and you were right that it used to. Everything below is
read from source in the current working tree — I ran nothing, so none of it is a runtime
observation.

## Where it stands now

The scan runs only if you tick a box, and the box starts unticked:
[timeline.html:756](timeline.html#L756), read at
[timeline.html:1334](timeline.html#L1334) and acted on at
[timeline.html:1384-1388](timeline.html#L1384-L1388). If it is unticked, nothing scans, and a
freshly uploaded export shows no automatic tags at all.

Upload no longer computes tags. `process_upload` parses, removes duplicates, stores one summary
per conversation, and stops — its own doc comment says so at
[backend/timeline-api/src/processing.rs:98-104](backend/timeline-api/src/processing.rs#L98-L104).
The scan lives in its own request, `POST /detect`
([backend/timeline-api/src/routes/detect.rs:72](backend/timeline-api/src/routes/detect.rs#L72)),
which the page calls in a loop of five conversations at a time so the progress bar moves on
something earned rather than a spinner
([timeline.html:1303-1329](timeline.html#L1303-L1329)).

Commit 6c7d9a9 ("Move detection out of upload into a user-triggered POST /detect") is the change;
the reasoning is written up in
[docs/plans/2026-09-28-frontend-quality-of-life.md:196-244](docs/plans/2026-09-28-frontend-quality-of-life.md#L196-L244),
under a heading that quotes your question.

## Why it used to

Not an oversight — a decision in
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md)
that the word-list pass costs about nothing per message and is "always available". What got built
read "always available" as "already computed at upload, whether or not anyone asked". Those are
different claims, and the second one is the one you objected to. The later plan says this plainly
at [lines 198-215](docs/plans/2026-09-28-frontend-quality-of-life.md#L198-L215), including that
the speed argument for the change was weak and unmeasured: the real reasons are that you should
not pay for work you did not ask for, and that you should be able to watch it happen.

## How it differs from "Classify with AI"

They are two separate passes over the same three tags, and they are genuinely different things:

| | the tick-box scan | "Classify with AI" |
|---|---|---|
| Runs where | in the Rust server, `POST /detect` | in the page, calling `https://api.anthropic.com/v1/messages` directly ([timeline.html:1745](timeline.html#L1745)) |
| How it decides | a dictionary check for shouted words, plus word lists and a sentiment score ([backend/timeline-api/src/processing.rs:84-90](backend/timeline-api/src/processing.rs#L84-L90)) | Claude Sonnet reads each message with the reply before it and judges ([timeline.html:1691](timeline.html#L1691), [timeline.html:1809](timeline.html#L1809)) |
| Which tags it sets | all three: shouting, criticism, anger | only criticism and anger; it never touches the shouting tag ([timeline.html:1847-1850](timeline.html#L1847-L1850)) |
| Money | none | real per-message cost; the page warns and asks first ([timeline.html:1816-1820](timeline.html#L1816-L1820)) |
| Where it works | anywhere the server is reachable | only while the page is open live as a Claude artifact; a downloaded copy of the file cannot reach the endpoint at all ([timeline-project-decisions.md:388-393](timeline-project-decisions.md#L388-L393)) |
| Where the result is kept | on the server, and from there into the file you download ([backend/timeline-api/src/routes/export.rs:85-105](backend/timeline-api/src/routes/export.rs#L85-L105)) | in the page and in browser storage only — it never reaches the server; the only thing the page sends back about tags is your own corrections ([timeline.html:1636](timeline.html#L1636)) |
| Where you trigger it | a box on the load screen | a box in the Review tab ([timeline.html:844](timeline.html#L844)) |

Both write the same automatic field and mark it with where it came from, so the Review table shows
"auto" for the word-list pass and "AI" for the Claude pass
([timeline.html:2479](timeline.html#L2479)). Neither one overwrites a tag you confirmed yourself.

## Your worry about confusing users is not fixed

The plan that made the scan opt-in says outright that it does not address this:
"[The user has **tabled** the question of how the two tiers are named and explained to users](docs/plans/2026-09-28-frontend-quality-of-life.md#L271)"
([docs/plans/2026-09-28-frontend-quality-of-life.md:271-277](docs/plans/2026-09-28-frontend-quality-of-life.md#L271-L277)).
It changed *when* the scan runs, and nothing else. So three things are still live:

1. **The two triggers sit in different places and neither mentions the other.** One is a tick box
   on the load screen, the other a box in the Review tab. Nothing tells you they produce the same
   three tags, or that one is free and rough while the other costs money and reads context.
2. **The download always says the tags came from the word lists.** The server has no field for
   where a tag came from — its stored record is just three true/false values
   ([backend/timeline-core/src/ports/message_flags.rs:33-37](backend/timeline-core/src/ports/message_flags.rs#L33-L37))
   — and the file it writes hard-codes `"source": "heuristic"`
   ([backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)).
   Since the Claude pass never reaches the server, its judgments live only in the page and in the
   file the Review tab downloads. Re-upload that file and the server, which never learned about
   them, can write plain word-list tags over them — so rows that said "AI" come back saying
   "auto". I read this from the code and did not run it.
3. **Corrections carried in an uploaded file are not read back in.** Nothing on the upload path
   reads the two embedded fields; a stored record starts with your corrections empty
   ([backend/timeline-storage/src/memory/message_flags.rs:73-87](backend/timeline-storage/src/memory/message_flags.rs#L73-L87)),
   and the download writes that empty record into the file
   ([backend/timeline-api/src/routes/export.rs:101-104](backend/timeline-api/src/routes/export.rs#L101-L104)).
   So the promise at [timeline.html:868-870](timeline.html#L868-L870) — "load that file directly
   next time and everything comes back automatically" — looks wrong for corrections made in an
   earlier session. Again: read, not run. This one is worth checking against a real upload before
   either of us believes it.

Items 2 and 3 are the kind of thing that will read to a user as "the tool lost my work", which is
worse than the naming confusion you raised. If you want, the next step I'd suggest is one plan
covering how the two passes are named and paired in the interface, plus reading the embedded
fields back in on upload so a re-uploaded file keeps both its source marking and your corrections.
```

---

## baseline-q2-r2

**Variant on disk (not what reached the instance):** baseline  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
## Short answer

It doesn't any more. In the code as it stands, nothing is detected unless you tick a box. But your
second point still stands: the box and the "Classify with AI" button do overlapping things, they sit
in different places, and almost nothing in the interface tells a user how they relate. Details below,
all read from the source — I ran nothing.

## Why it used to always detect

It was a decision, not an oversight, and it came from the wrong reading of one line. The migration
plan records a confirmed pricing split at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:32](docs/plans/2026-09-09-rust-aws-backend-migration.md#L32):
a free tier of word-list detection (ALL-CAPS with a dictionary check, keyword and sentiment
criticism/anger) that is "always available", versus one paid Claude-quality pass per $5. "Always
available" got built as "already computed for everything, at upload, whether or not anybody asked."
Those are different claims, and the second one is what you objected to.

## What it does now

Upload no longer computes anything.
[backend/timeline-api/src/processing.rs:98](backend/timeline-api/src/processing.rs#L98) says so in
the docstring on `process_upload`, and the body confirms it: parse, dedup, write one summary per
conversation, record the outcome, return. No flag writes.

Detection moved to its own route, `POST /detect`
([backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)), added in
commit 6c7d9a9 "Move detection out of upload into a user-triggered POST /detect". It is reached only
from an unchecked-by-default checkbox on the load screen
([timeline.html:756](timeline.html#L756)), read at [timeline.html:1334](timeline.html#L1334) and
acted on at [timeline.html:1384](timeline.html#L1384). The page drives it a few conversations at a
time in `runDetectionPass` ([timeline.html:1303](timeline.html#L1303)) so the progress bar shows
real progress rather than a spinner. Two of the browser tests cover exactly this: "uploading without
asking for detection produces no flags at all"
([e2e/views.spec.js:318](e2e/views.spec.js#L318)) and "the detection pass reports progress while it
runs" ([e2e/views.spec.js:331](e2e/views.spec.js#L331)). I did not run them, so that is source I
read, not behaviour I watched.

## How the box differs from the "Classify with AI" button

They are genuinely two different things, and here is every difference I can find in the code.

| | Load-screen checkbox | "Classify with AI" button |
|---|---|---|
| Where it lives | Load screen, [timeline.html:756](timeline.html#L756) | Review & flags tab, [timeline.html:844](timeline.html#L844) |
| Where the work happens | Rust, on the server | In your browser, calling `api.anthropic.com` directly ([timeline.html:1685](timeline.html#L1685)) |
| How it decides | Word lists and a sentiment score — `heuristic_flags` at [backend/timeline-api/src/processing.rs:84](backend/timeline-api/src/processing.rs#L84) | Asks Claude (Sonnet) per message, with the preceding Claude reply as context ([timeline.html:1809](timeline.html#L1809)) |
| What it sets | All three: ALL-CAPS, critical, angry | Critical and angry only. Never touches ALL-CAPS ([timeline.html:1848](timeline.html#L1848)) |
| Cost | No token cost | Sends every message you ever sent, in batches of 30 ([timeline.html:1690](timeline.html#L1690)) — real tokens |
| Where the result is stored | The backend, so it comes back in the export and survives a reload | In the page's memory only. `patchFlagsToBackend` ([timeline.html:1624](timeline.html#L1624)) is called from one place — your own manual corrections at [timeline.html:1668](timeline.html#L1668) — never from the AI pass |
| Works outside a live Claude artifact | Yes | No ([timeline.html:1598](timeline.html#L1598)) |

What they share: both write into the *same* slot. Both set the "automatic" values, and neither can
touch a flag you confirmed yourself. So running one after the other overwrites the other's answer for
critical and angry, and running the AI pass leaves whatever the word lists said about ALL-CAPS
untouched.

## Why this will still confuse users — the specific places it leaks

1. **The two triggers never mention each other.** The checkbox copy
   ([timeline.html:756-766](timeline.html#L756-L766)) and the button copy
   ([timeline.html:835-843](timeline.html#L835-L843)) each describe themselves well. Neither says
   the other exists, that they compete for the same two flags, or which wins. The button's copy does
   say it is "meant to be more accurate than the keyword/sentiment heuristic below" — which points at
   a thing the user has no name for, because the checkbox that produced it was three screens ago and
   was not called that.

2. **The only visible difference afterwards is a two-or-three-letter label.** Each Review row shows
   `auto`, `AI`, or `you` ([timeline.html:2479](timeline.html#L2479)), with the longer explanation
   only in a hover tooltip ([timeline.html:2462](timeline.html#L2462)). The flag key in the header
   and the "Show automatic tags" toggle both treat "automatic" as one thing.

3. **The AI's answers quietly evaporate.** They are not saved to the backend (row 7 above). They are
   checkpointed to artifact storage, but the load-time recovery of that checkpoint was retired
   ([timeline.html:1590-1600](timeline.html#L1590-L1600)). So a reload drops them and the rows go
   back to saying `auto`, with no notice. A user who spent tokens on the pass loses it by refreshing.

4. **Downloading and re-uploading can silently revert the AI's answers.** The download does record
   which source produced each flag ([timeline.html:1928](timeline.html#L1928)). The server's export
   hardcodes `"source": "heuristic"`
   ([backend/timeline-api/src/routes/export.rs:99](backend/timeline-api/src/routes/export.rs#L99)) —
   the backend has no concept of an AI-sourced automatic flag at all. The export only overwrites a
   message's automatic block when the backend has a stored record for it
   ([export.rs:89-105](backend/timeline-api/src/routes/export.rs#L89-L105)), so: re-upload your
   annotated file *without* ticking the box and the AI's answers survive; re-upload *with* the box
   ticked and the word lists overwrite them and the label flips back to `auto`. Same file, two
   different outcomes, decided by a checkbox on a different screen. This is the one I would fix
   first, because it loses paid-for work without saying anything.

5. **The naming of the two tiers is deliberately unresolved.** Phase 4 of
   [docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
   records that you tabled how the two are labelled and explained, to design separately, and that
   the phase therefore changed only *when* the pass runs. So points 1 and 2 are known-open by
   design; points 3 and 4 I don't see recorded anywhere, and they look like gaps rather than
   decisions.

## Verified versus not

Read in source: everything above. Not observed: I did not start the backend, run the browser tests,
or click either trigger, so the reverting behaviour in point 4 is traced through three files, not
watched happening. The cheapest way to confirm it is one browser test that runs the AI pass against
a stubbed endpoint, downloads, re-uploads with the box ticked, and asserts the label — say so and
I'll write it.
```

---

## baseline-q2-r3

**Variant on disk (not what reached the instance):** baseline  
**Question:** Why does the backend always detect? It's not clear to me whether that is different from the "Classify with AI" button available elsewhere in the interface, and it will certainly confuse users.

```
Short version: it doesn't any more. Reading the code as it stands today, the backend computes
automatic tags only when the user ticks a box. But you are right that the two triggers are
different things, and right that the page as written will confuse people — I found five specific
places where it already does, including two lines of copy that are simply false.

## It used to always detect, and no longer does

`process_upload` explicitly does not compute flags — the doc comment says so at
[backend/timeline-api/src/processing.rs:99-105](backend/timeline-api/src/processing.rs#L99-L105):
"A freshly uploaded export therefore has no automatic flags until detection is requested, which is
the intended behavior and not a missing write." The pass lives in its own route,
`POST /detect` ([backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs),
wired at [backend/timeline-api/src/app.rs:27](backend/timeline-api/src/app.rs#L27)), which the page
only calls when the load-screen checkbox is checked
([timeline.html:1334](timeline.html#L1334) reads it,
[timeline.html:1384-1388](timeline.html#L1384-L1388) acts on it). The box is unchecked by default
([timeline.html:756](timeline.html#L756)). There are tests named for exactly this — one asserting an
upload has no automatic flags until detection is asked for
([backend/timeline-api/tests/detect.rs:166](backend/timeline-api/tests/detect.rs#L166)) and a browser
test asserting that uploading without asking produces no flags at all
([e2e/views.spec.js:318](e2e/views.spec.js#L318)).

I have read this in source and in test names; I have not run anything, so treat "the code says it
only runs on request" as observed and "it therefore behaves that way in a live deployment" as
inferred.

**Why it used to.** Not an oversight. The confirmed decision at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:31-33](docs/plans/2026-09-09-rust-aws-backend-migration.md#L31-L33)
was a free tier of word-list and sentiment detection "always available," against one paid
Bedrock-quality pass per $5. What got built read "always available" as "already computed at upload,
whether or not anyone asked." Phase 4 of
[docs/plans/2026-09-28-frontend-quality-of-life.md:196](docs/plans/2026-09-28-frontend-quality-of-life.md#L196)
is headed with your question verbatim, separates those two claims, and records the change as done.

## Yes, it is a different thing from "Classify with AI"

Six differences, all of them read out of the code rather than the prose:

1. **What decides.** The checkbox runs `heuristic_flags`
   ([backend/timeline-api/src/processing.rs:84-90](backend/timeline-api/src/processing.rs#L84-L90)):
   an ALL-CAPS check against a dictionary of acronyms, plus keyword and sentiment scoring. The
   button sends your message text, with Claude's preceding reply for context, to Claude Sonnet for a
   zero-shot judgment ([timeline.html:1685-1691](timeline.html#L1685-L1691)).
2. **Which of the three flags it can set.** The checkbox sets all three — caps, critical, angry. The
   button sets only critical and angry and leaves caps untouched
   ([timeline.html:1845-1851](timeline.html#L1845-L1851)).
3. **Where the work happens.** The checkbox is server work, paged five conversations at a time so
   the bar can move ([timeline.html:1303-1328](timeline.html#L1303-L1328),
   [backend/timeline-api/src/routes/detect.rs:126-131](backend/timeline-api/src/routes/detect.rs#L126-L131)).
   The button is the browser calling api.anthropic.com itself, which only has a route to that
   address while this page is running live as a Claude artifact — a locally-opened copy of the file
   will fail every batch ([timeline.html:1685-1689](timeline.html#L1685-L1689)).
4. **Whether the result survives.** Detection writes through the backend's automatic-flag writer, so
   it comes back on the next load. The button's results are only in the page's memory plus a cache
   in the artifact's own storage that is no longer read back at load time
   ([timeline.html:1598-1600](timeline.html#L1598-L1600)). The one flag write the page makes to the
   backend is the user's own override at [timeline.html:1636](timeline.html#L1636) — nothing sends
   the Claude-judged values there. Their only durable route out is "Download annotated
   conversations.json," which embeds them with a source marker
   ([timeline.html:1928](timeline.html#L1928)).
5. **Cost.** One is dictionary lookups on your own server. The other spends tokens on every message,
   which is why it asks first ([timeline.html:1816-1820](timeline.html#L1816-L1820)).
6. **What they have in common,** and the page does say this in both places: neither touches a flag
   you confirmed yourself.

## Where the interface will actually confuse someone

Naming the two tiers was deliberately left out of Phase 4 — you tabled it — so the following are the
concrete collisions that exist in the page right now, not a proposal for names.

- **The two triggers never mention each other.** The checkbox is on the load screen
  ([timeline.html:754-766](timeline.html#L754-L766)); the button is in the Review tab
  ([timeline.html:844](timeline.html#L844)). Nothing tells you they do overlapping jobs, or that one
  overwrites the other's critical and angry values while leaving its caps values in place.
- **The Review-tab copy compares the button to something that may never have run.**
  [timeline.html:838](timeline.html#L838) sells it as "more accurate than the keyword/sentiment
  heuristic below." Since the box is unchecked by default, a first-time user reads a comparison
  against an empty table.
- **A message with no automatic tag is labelled "auto."** The marker at
  [timeline.html:2479](timeline.html#L2479) prints "AI" when the source is Claude and "auto"
  otherwise — and "otherwise" now includes the case where no detection ever ran, because Phase 4
  made that the normal state. Three states are being shown with two labels, and the third one is
  wearing the first one's badge. The hover text at
  [timeline.html:2462](timeline.html#L2462) is correct and simply absent in that case.
- **The load screen still says the file goes nowhere.**
  [timeline.html:743](timeline.html#L743): "nothing is uploaded anywhere." The export is uploaded.
  Phase 3 of the frontend plan flagged this line as needing correction regardless; it is still
  there.
- **The footer still says the backend always computes the tags.**
  [timeline.html:892-893](timeline.html#L892-L893): "uploaded to your timeline backend, which stores
  it and computes the automatic tags." After Phase 4 that is only true if you ticked the box. This
  one is the direct textual residue of the behavior you asked about, and it now contradicts the
  checkbox text three hundred lines above it.

The last two are worse than the naming question, because a user can't correct for copy that states
the wrong fact. I'd fix those two lines before touching anything else; they are single-sentence
edits and they don't depend on how you eventually decide to name the two tiers.
```

---

## baseline-q3-r1

**Variant on disk (not what reached the instance):** baseline  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the work left off, and what is verified

Everything below comes from reading files and git history. I ran no tests and no code, so every
claim about whether something *works* is a claim about what a document or a source file says, not
something I watched happen. I flag which is which.

## Where it left off

Two different kinds of work, one after the other.

**The last change to the product** was on 29 September: `POST /_dev/reset`, a local-development-only
route that empties the in-memory stores, plus a test that the routes under `/_dev/` stay out of the
build that would be deployed. Commits `da1457a`, `d725cc3`, `7cb86d9`, `d81b89e`. That finished the
six-phase list in
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md),
whose own status table marks all six **Done** ([lines 10-17](docs/plans/2026-09-28-frontend-quality-of-life.md#L10-L17)).
That plan says these were the things to finish *before* going back to the backend migration, so the
next piece of product work is the migration plan's unstarted item: the real Amazon S3 and DynamoDB
adapters, tracked as C10 at
[docs/plans/2026-09-09-rust-aws-backend-migration.md:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926).

**Since then, 30 September, the work has not been product work at all.** It has been an experiment
about which written instruction makes me use fewer words the reader does not already share:

- [docs/plans/2026-09-30-vocabulary-instruction-experiment.md](docs/plans/2026-09-30-vocabulary-instruction-experiment.md) — the first design.
- [docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md) — what came of it. 2 runs per condition out of a target of 10; the first 20 runs were lost entirely.
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md) — a second design, 7 conditions x 10 runs.

**The working tree is not clean.** Two files carry uncommitted edits: one line of
[CLAUDE.md](CLAUDE.md) ("on an already-approved plan"), and a revision to the second experiment
design that replaces two of its three questions.

**The second experiment is part-run and its data is not on disk.** The uncommitted revision says two
retired questions are "recorded in
[the results file](docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md)"
([line 61](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md#L61)), and quotes a
1,156-second run — so runs have happened. That file does not exist;
[docs/analysis/](docs/analysis/) holds only the first experiment's results. So there are observations
being cited that nothing in the repository can back up. That is the loudest loose end I found.

## Verified — by someone running the code, per the record

I am reporting what [backend/README.md:91](backend/README.md#L91) and the plans claim was run. I did
not re-run any of it.

- **The web server over real HTTP requests.** Rejecting requests with no credentials and with
  garbage credentials, issuing an upload URL, listing an empty account, and setting then reading a
  message's flags. Held as committed tests in
  [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs) driving the real router.
- **Token checking**, including every rejection path, against a real signing key generated at
  run time — [backend/timeline-auth/tests/cognito.rs](backend/timeline-auth/tests/cognito.rs).
- **The in-memory stores**, including the rule that automatic flags and the user's own corrections
  cannot overwrite each other — [backend/timeline-storage/tests/](backend/timeline-storage/tests/).
- **The whole page against the whole server, in a real browser.** Two Playwright files:
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) (3 tests — upload and render, a file above the
  2MB limit, a confirmed flag surviving a reload) and
  [e2e/views.spec.js](e2e/views.spec.js) (13 tests — calendar, transcripts, each analytics view,
  search and paging, the annotated download, detection running only when asked, both progress bars,
  the location bar and Back, session restore). The recorded result of the last run,
  [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json), says `passed` and is
  timestamped today 04:14 — that is a file I read, not a run I did.
- **The deployable build excludes the local-only routes.**
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  what the deployed entry point builds and asserts every `/_dev/*` path is absent while the real
  routes are present. This matters because one of those routes mints a token with no password and
  another erases everything.
- **The cross-compiled deployable binary** was built and driven by hand through a local stand-in for
  the Amazon Lambda runtime, with four request shapes answered correctly. Recorded at
  [backend/README.md:113-135](backend/README.md#L113-L135) as a manual session — **not a committed
  test**, so nothing re-checks it.
- **Deleting the page's own copy of the flag detection** was checked to leave the rendered page
  byte-for-byte identical ([the phase table](docs/plans/2026-09-28-frontend-quality-of-life.md#L14)).

## Not verified

- **Nothing has ever talked to Amazon.** [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs)
  and [backend/timeline-storage/src/dynamo/](backend/timeline-storage/src/dynamo/) compile, and the
  request-building parts have tests, but no call in either file has reached a real service or a local
  stand-in. Stated at [backend/README.md:135](backend/README.md#L135), unchanged as C10 at
  [the migration plan:926](docs/plans/2026-09-09-rust-aws-backend-migration.md#L926). Everything
  working today runs on stores that live inside the server process, so **restarting the server
  deletes every upload, conversation and flag.**
- **[infra/template.yaml](infra/template.yaml) has never been checked or deployed.**
- **No real user accounts.** Sign-in is a name typed into a box; there is no real account service, and
  nothing has been tested against one.
- **Coverage is 100% only in [backend/timeline-core](backend/timeline-core/); the other three crates
  fall short**, concentrated in the untested Amazon calls
  ([backend/README.md, "Test coverage"](backend/README.md#L150)).
- **The browser tests are not part of `cargo test --workspace`** — different toolchain — so they only
  run when someone runs them deliberately ([e2e/README.md](e2e/README.md)).

## Three places where the documents and the code disagree

I checked each of these in the source rather than trusting the write-up.

1. **C12 is marked RESOLVED and I could not find its resolution.**
   [Line 444](docs/plans/2026-09-28-frontend-quality-of-life.md#L444) claims "one command runs every
   suite — the Rust workspace, the Playwright e2e suite, and the launcher test", plus a check for
   exceptions that get discarded silently, in both Rust and JavaScript. What I found: no such command
   anywhere — [scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh`,
   `test-dev-up.sh`; there is no Makefile or equivalent; [.vscode/tasks.json](.vscode/tasks.json) has
   a task for the launcher test only. No file matching that exception check exists, in any language.
   The one ratchet that does exist,
   [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs), walks only
   `.rs` files under [backend/](backend/) — which the plan itself records as still remaining. So this
   entry claims work that the repository does not contain. I would treat it as OPEN.
2. **[backend/README.md:216](backend/README.md#L216), "What's not built yet", is out of date.** It
   says the upload-processing step, the annotated download, and the page's wiring to the server are
   not built. All three exist: [backend/timeline-api/src/processing.rs](backend/timeline-api/src/processing.rs),
   [backend/timeline-api/src/routes/export.rs](backend/timeline-api/src/routes/export.rs), and a
   [timeline.html](timeline.html) that calls the server throughout. The same staleness sits at
   [the migration plan:281](docs/plans/2026-09-09-rust-aws-backend-migration.md#L281), which says the
   page half "is not started".
3. **The page still tells the user "nothing is uploaded anywhere"**
   ([timeline.html:743](timeline.html#L743)). That is no longer true, and the migration plan flagged
   at [line 26](docs/plans/2026-09-09-rust-aws-backend-migration.md#L26) that this wording had to be
   rewritten as soon as the tool started uploading. It was not.

## Open items already written down

- **Two known defects, both still unfixed** ([line 466](docs/plans/2026-09-28-frontend-quality-of-life.md#L466)).
  First: conversations come back in a different order after every server restart, so the list and the
  per-conversation colours reshuffle. I confirmed the cause is still there —
  [backend/timeline-storage/src/memory/conversations.rs:36-48](backend/timeline-storage/src/memory/conversations.rs#L36-L48)
  walks a hash map and collects without sorting, and Rust randomises that order per process. Second:
  the page intermittently reports it cannot reach the server while the server is answering; worked
  around by waiting for the port, never explained.
- **C1, still open**: reading the export blocks the page, so that phase cannot show real progress.
  Revisit only after measuring whether it still visibly freezes.
- **C8, still open**: whether restoring the last session on open should be silent. It currently
  announces itself, which the plan says was your call.
- **Tabled by you**: how the two tiers of emotion detection are named and explained to users. No
  labels were invented while it is tabled.
- **C6 in the migration plan, still open**: capturing one real inference call needs your sign-off to
  briefly enable it on a real account, and that gates the classification work.
- **Only [e2e/views.spec.js](e2e/views.spec.js) clears the server between tests.** Commit `d81b89e`
  is titled "Reset the backend before each e2e test" and touched that one file;
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) has no such step, and both files share one
  server on one port. I have not traced whether that can bite — upload-flow's tests run first by
  filename order — but the title claims more than the change delivers.

## Shortest description

The tool works end to end in a real browser against a server that keeps everything in memory, and
that much is covered by 16 committed browser tests plus the Rust suites. Nothing has been deployed,
nothing has touched Amazon, nothing persists across a restart, and there is no real sign-in. Product
work stopped a day ago and attention moved to an experiment about my own writing, whose second round
is part-run with its data missing. Before more code, three write-ups need correcting to match the
code, and the claim that "one command runs every test" needs either building or marking open.
```

---

## baseline-q3-r2

**Variant on disk (not what reached the instance):** baseline  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where the conversation-timeline project left off, and what is checked

Everything below comes from reading the repository and its documents. I ran no tests and changed no
code, so where a claim rests on someone else's earlier run rather than on something I saw happen, I
say so.

## The short answer

The browser-facing work is finished and the writing work is what is in progress. All six phases of
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
are marked done. The last four commits are not code at all — they are an experiment about which
written instruction keeps me from using words the user has not used. The stopping point is that the
second version of that experiment's plan is written and committed but has not been run.

## The last thing touched

```
966655d  Plan v2 of the vocabulary-instruction experiment      <- newest
57c8d3b  Fix two statements the timeout diagnosis contradicted
54c9e19  Diagnose the slow runs: a fixed 600-second timeout
af47dae  Write up the vocabulary-instruction experiment and its results so far
7cb86d9  Mark C11 resolved: the Lambda build is now tested for _dev absence   <- last code commit
```

Two files are edited and not committed:

- [CLAUDE.md](CLAUDE.md) — one sentence, "an already-approved plan" in place of "a plan".
- [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
  — retires two of the three questions (the two that made instances run the test suite and write
  measuring programs), puts three questions that can be answered by reading in their place, and adds
  one line appended to every question telling the instance to read only. A round of plan editing not
  yet committed.

## What is built

**The Rust backend and the page are wired together and work as a pair.** Upload a file, it is
deduplicated, split into sessions, and comes back rendered; flags confirmed in the review table
reach the backend and survive a reload. Storage is in memory only, so restarting the server deletes
everything.

**The page shrank by 85%.** [timeline.html](timeline.html) is now 3,110 lines / 119,073 bytes. The
plan measured it at 66,839 lines / 752,370 bytes before the deletion; most of that was an embedded
English word list and a sentiment word list feeding detection the server now does. The plan records
the deletion as verified to change nothing on screen, byte for byte.

**Detection now runs only when asked.** [backend/timeline-api/src/routes/detect.rs](backend/timeline-api/src/routes/detect.rs)
took the pass out of the upload path; the page drives it in pages so the progress bar means
something.

**Not built at all**, per the migration plan's own version list in
[docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md):
the Amazon Bedrock classification pass (V3), the $5 charge and the gating it pays for (V4), and the
load/security hardening (V5). The upload-processing function that S3 was supposed to trigger, and
`GET /export`'s own AWS resources, are missing from
[infra/template.yaml](infra/template.yaml) — its own header says so.

## Checked by running the code

- **The Rust test suite.** Two documents disagree on its size:
  [backend/README.md](backend/README.md) says `cargo test --workspace` runs 163 tests; the coverage
  audit written into the quality-of-life plan says 173 tests pass at 77.57% line coverage, measured
  with `cargo llvm-cov`. I did not run either, so I cannot say which number is current — the
  difference is 10 tests and one of the two documents is stale.
- **The browser tests.** 21 cases across
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) (3) and
  [e2e/views.spec.js](e2e/views.spec.js) (18, because one declaration runs over five analytics
  views). They drive real Chrome against a real `timeline-api` the suite starts itself.
  [e2e/test-results/.last-run.json](e2e/test-results/.last-run.json) records `"status": "passed"`
  with no failures, and the file is dated 30 September 04:14. That is a saved record of somebody's
  run, not something I watched happen, and it does not name the commit it ran against.
- **The local server by hand, over real HTTP**, with the results kept as committed tests in
  [backend/timeline-api/tests/app.rs](backend/timeline-api/tests/app.rs): requests with no token
  and with a junk token rejected, `POST /uploads` answering with a real signed-URL-shaped
  response, a flag written then read back.
- **Token checking** in [backend/timeline-auth/src/cognito.rs](backend/timeline-auth/src/cognito.rs),
  including every rejection path that matters for security, against a real self-signed key.
- **Cross-compiling for AWS Lambda.** `cargo lambda build --release --arm64` produced a real ARM64
  binary on an x86-64 machine, and `cargo lambda watch` plus `cargo lambda invoke` fed it genuine
  API-Gateway-shaped events; four requests came back correct. This was a by-hand session, recorded
  in [backend/README.md](backend/README.md) but not captured as a test that reruns.
- **The dev-only routes cannot reach the deployable build.**
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  exactly what the Lambda branch builds, asserts every `/_dev/*` path answers 404, and — so the test
  could not pass against an empty router — asserts the real routes are present and merely refused.
  This matters because `POST /_dev/login` mints tokens for anyone and `POST /_dev/reset` erases
  everything.

## Not checked, and the documents say so

- **The real S3 and DynamoDB code has never talked to anything.** No `send()` call in
  [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) or
  [backend/timeline-storage/src/dynamo.rs](backend/timeline-storage/src/dynamo.rs) has ever reached
  AWS or a local stand-in. This is the migration plan's C10, and it is open with no work started:
  AWS publishes a downloadable DynamoDB Local that needs no container, but no equally clean
  container-free option for S3 was found (MinIO's licence changed).
- **[infra/template.yaml](infra/template.yaml) has never been validated or deployed.** No AWS
  credentials and no SAM tool in this environment.
- **Nothing has been tried against a real Cognito user pool** — only against a self-signed key
  standing in for one. Cognito's current free monthly-user allowance is also an unchecked item in
  the plan.
- **The built Lambda binary has never been deployed to real AWS Lambda.**

## Two things a plan calls resolved that I could not find in the repository

This is the widest gap between what the documents claim and what is on disk, so it is here rather
than in a footnote.

**C12 in [docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
is tagged [RESOLVED] and promises two things that do not exist.** Its resolution says "one command
runs every suite — the Rust workspace, the Playwright e2e suite, and the launcher test", pinning the
Node version, plus "a silent-exception-swallow check covering both the Rust and the JavaScript".
What I found instead:

- [scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh` and
  `test-dev-up.sh`. No script runs more than one suite.
- [.vscode/tasks.json](.vscode/tasks.json) has no such task.
- [e2e/README.md](e2e/README.md) still says the opposite: "Nothing here runs as part of
  `cargo test --workspace` — different toolchain entirely. Treat it as a required manual step."
- No check for exceptions that get thrown away silently exists in any language. There is no Python
  in this repository at all, so the `tests/test_no_unhandled_exceptions.py` that
  [CLAUDE.md](CLAUDE.md) describes is not here either.
- The commit that wrote that resolution, `4a8538b`, changed the plan file and nothing else.

So C12 records a decision about what to do, tagged as though it were done. Running "all tests" today
still means running three things by hand in three places.

**The file-length check reaches only Rust.**
[backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs) walks
`backend/` and keeps only files ending in `.rs`, so [timeline.html](timeline.html) is outside it.
The plan says this remains, and it does.

## Known defects still open

1. **Conversation order changes between server restarts.** Confirmed still unfixed:
   `list_for_user` in
   [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
   collects from a hash map and never sorts, and Rust randomises that order per process. A user sees
   the conversation list and its colours reshuffle. The detection route works around it by sorting
   by conversation id itself, and its comment explains why, so the underlying fault is documented
   but not fixed.
2. **The page intermittently reports it cannot reach the backend while the backend is answering.**
   Not traced to a cause; worked around by waiting for the server before driving the browser.
   Alongside this sits a contradiction I noticed and cannot settle by reading:
   [README.md](README.md) tells users that opening the page as a local file "does not reliably work
   ... empirically confirmed", while both browser test files open it exactly that way, with no
   special browser switches in [e2e/playwright.config.js](e2e/playwright.config.js), and the last
   recorded run passed. One of those two statements is wrong, or the difference has a cause nobody
   has found.
3. **C1, open:** the page cannot show progress while it parses the uploaded file, because that work
   blocks the one thread that draws. Its trigger for revisiting was "measure again once the file is
   smaller" — the file is now smaller, so that measurement is due, and I found no record of it.
4. **C6 and C8 in the migration plan, both open:** capturing genuine recorded exchanges with AWS as
   test material needs real calls; and the deduplication statistic in the decisions document still
   does not match the real export, with the plan stating plainly that it was never confirmed
   against the original 4,482-message file.

## Where the experiment stands

The first attempt is written up in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md).
It aimed for 10 runs per condition and completed 2, across three conditions. An earlier batch of 20
was lost entirely when the session that launched them ended. The findings it does support: the
strictest word rule scored best on its own measure and produced the worst writing; the measure
itself cannot tell a term of art from the word *whose*; and the rule as written into
[CLAUDE.md](CLAUDE.md) forbids *but* on a literal reading, which an instance obeying it demonstrated.
Two runs also took 32 and 33 minutes, which turned out to be three blocked calls of exactly 600
seconds each and not the instruction's fault; with that subtracted, the strict rule still costs four
to five times the working time.

The second version fixes all of that on paper: seven conditions, ten runs each, questions taken
from the user's own words, and a scoring rule that exempts ordinary English. It has not been run.
By the workflow in [CLAUDE.md](CLAUDE.md) it is waiting for the user to say whether the plan is
acceptable.

One small thing to fix in that plan: it links "the results file" as
`docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md`, which does not exist. The first
attempt's results are at
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md);
the name it links is one the second run has not produced yet.

## Two other documents that have drifted

- [e2e/README.md](e2e/README.md)'s "What's covered" section describes only
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js). The 18 cases in
  [e2e/views.spec.js](e2e/views.spec.js) — calendar, opening a conversation, five analytics views,
  review search and paging, export, detection on demand, upload progress, browser Back, and session
  restore — go unmentioned.
- [backend/README.md](backend/README.md)'s test count, as above.

## If you want one next step

Land C12 for real, because it is the one open item that makes every other verification claim harder
to trust: one command that runs the Rust suite, the browser suite and the launcher test, and a check
that no exception is thrown away without being logged. Everything else that is open is either
waiting on AWS access (C10, the template, Cognito) or waiting on your say-so (the experiment's plan,
the names for the two detection tiers).
```

---

## baseline-q3-r3

**Variant on disk (not what reached the instance):** baseline  
**Question:** Where did work on this project leave off, and what is verified versus what is not?

```
# Where work left off, and what is verified versus what is not

I answered by reading the repository and its plan and analysis documents. I ran nothing. Where a
claim below comes from a document rather than from source I read or a run I observed, I say so.

## Where work left off

Two separate tracks are in flight.

**The product.** The last commit that touched the product is `7cb86d9`, "Mark C11 resolved: the
Lambda build is now tested for _dev absence". Everything committed after it is about a writing
experiment, not the tool. So the product has been parked since that commit.

The six-phase frontend pass in
[docs/plans/2026-09-28-frontend-quality-of-life.md](docs/plans/2026-09-28-frontend-quality-of-life.md)
is marked **Done** in all six rows: the dev-server launcher, the browser-test coverage for the
calendar and analytics views, the deletion of the page's own flag-detection code and its two word
lists, detection behind an explicit trigger with visible progress, the upload progress bar, and
back/forward navigation. That plan's own header says these were the things to finish *before*
returning to the migration plan's server work — naming `C10`, the real S3 and DynamoDB adapters, as
what comes next. That is the intended next step.

In [docs/plans/2026-09-09-rust-aws-backend-migration.md](docs/plans/2026-09-09-rust-aws-backend-migration.md),
V1 (pure Rust logic), V2's backend crates, and V2a (wiring `timeline.html` to the backend for both
reading and writing flag overrides) are landed. V3 (classification through Amazon Bedrock), V4
($5 charge and gating), and V5 (load, security, chaos) are unstarted — the crates that plan's §6.1
assigns to them, `timeline-bedrock` and `timeline-payments`, do not exist in the tree.

Everything the running backend stores is in memory. Restarting `cargo run -p timeline-api` deletes
every upload, conversation and flag. That is deliberate, stated plainly in both the plan and
[README.md](README.md), not a defect.

**The writing experiment.** A side investigation into which written instruction reduces vocabulary a
reader has not already seen. Its first attempt is written up in
[docs/analysis/2026-09-30-vocabulary-instruction-results.md](docs/analysis/2026-09-30-vocabulary-instruction-results.md):
20 runs were launched and all 20 lost when the coordinating session ended, then 6 ran and completed —
2 each for 3 of the 7 candidate instructions, against a target of 10 each. It was stopped early, and
correctly so: running the candidate found a real defect in it (read literally, the rule forbids
ordinary words like *but* and *not*, and the copy of that rule in
[CLAUDE.md](CLAUDE.md) has the identical flaw), and the scoring measure was found unfit — it cannot
tell *hash-keyed* from *whose*. A second design is committed at
[docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md)
(70 runs, 7 measurements). **It has not run**: none of the three files it says it will produce exist.

**Uncommitted in the working tree**, two files:
- [CLAUDE.md](CLAUDE.md) — one line changed, "finish work on a plan" to "finish work on an
  already-approved plan".
- The v2 experiment plan — a substantial edit that retires two of its three questions. Both had made
  instances do work instead of reading: one asked for the test suite's duration, so every instance
  ran the suite, ten at once against one machine and one port; another asked about re-parsing cost,
  and two instances wrote benchmark programs. The edit replaces them and appends one constant line to
  every question telling the instance to answer by reading only.

## What is verified

All of these are test files I read in the repository. I did not run them, so "verified" here means
the coverage exists and is committed, not that I watched it pass today.

- **The pure logic** in `timeline-core` — deduplication, unwrapping the export, session blocks, the
  ALL-CAPS check, criticism and anger detection, the four-state flag matrix. Covered by hand-built
  edge cases, a property test, a snapshot, and a regression test against a real sample export
  checked in at
  [backend/timeline-core/tests/fixtures/sample_conversations.json](backend/timeline-core/tests/fixtures/sample_conversations.json).
- **The in-memory storage adapters** — one test file each for uploads, conversations, message flags
  and the object store.
- **The API routes** — request-and-response level tests per route, plus `process_upload` driven
  through the real port methods against the in-memory fakes.
- **The deployable router excludes the development-only routes.**
  [backend/timeline-api/tests/lambda_router.rs](backend/timeline-api/tests/lambda_router.rs) builds
  exactly what the Lambda branch builds, asserts every `/_dev/*` path returns 404, and — so the test
  could not also pass on an empty router — asserts the real routes are present and merely
  unauthorized. This is the one property where being wrong would mean shipping an endpoint that
  mints tokens without a password, alongside a `POST /_dev/reset` that erases everything.
- **The browser flow** — two Playwright suites,
  [e2e/upload-flow.spec.js](e2e/upload-flow.spec.js) and [e2e/views.spec.js](e2e/views.spec.js),
  drive real Chrome against a real running `timeline-api`, and reset the backend before each test.
- **The launcher's decision to skip or restart** — [scripts/test-dev-up.sh](scripts/test-dev-up.sh).
- **The Phase 3 deletion changed no output.** The plan records this as verified byte-for-byte. That
  is the document's claim; I did not reproduce the comparison.

## What is not verified

The three real AWS adapters say so themselves, in their own opening comments, which is the right
place for it:

- **S3.** [backend/timeline-storage/src/s3.rs](backend/timeline-storage/src/s3.rs) has zero test
  coverage and has never been run against real or emulated S3 — no credentials, no Docker in the
  environment it was written in.
- **DynamoDB.** [backend/timeline-storage/src/dynamo.rs](backend/timeline-storage/src/dynamo.rs) and
  its two table adapters have their key-and-expression building unit-tested, and have never been run
  against DynamoDB Local or real DynamoDB.
- **Cognito.** [backend/timeline-auth/tests/cognito.rs](backend/timeline-auth/tests/cognito.rs)
  proves the verification logic against a throwaway keypair generated at runtime. There is no real
  user pool in this environment, and the test's own header says checking against a real pool's tokens
  is still needed before V2 can be called done.
- **The infrastructure template.** [infra/template.yaml](infra/template.yaml) has never been
  deployed, and never even checked with `sam validate`. There is no `samconfig` and no deployment
  record anywhere in the tree.

So nothing in this project has ever run on AWS. By its own V2 test list, V2 is not done: the
emulator-backed integration run, a roughly 60MB large-payload run, and one run against real low-cost
S3 and DynamoDB are all outstanding — even though V2's code is in and the phases built on top of it
are finished.

Two documentation statements I could not confirm against the tree, which I think matter more than the
rest of this list because they are marked as settled:

1. **The single command that runs every test suite does not appear to exist.** The `C12` entry in the
   frontend plan is marked `[RESOLVED]` and says "one command runs every suite — the Rust workspace,
   the Playwright e2e suite, and the launcher test — and that command is what 'run all tests' means."
   I looked for it: [scripts/](scripts/) holds only `dev-up.sh`, `dev-down.sh`, `port-control.sh` and
   `test-dev-up.sh`; there is no `Makefile` or `justfile`; [.vscode/tasks.json](.vscode/tasks.json)
   has a task for the launcher test but none that runs everything; and the only script in
   [e2e/package.json](e2e/package.json) is `playwright test`. Running all three still takes three
   commands from three directories, with a Node version switch for one of them. The same `C12` block
   also puts a silent-exception check for both Rust and JavaScript in scope; the only ratchet test
   present is [backend/timeline-core/tests/file_sizes.rs](backend/timeline-core/tests/file_sizes.rs),
   which checks Rust file sizes. Unless this landed somewhere I failed to find, that entry describes
   a fix that was not applied, and the status tag overstates it.
2. **The infrastructure template's own comment is now stale.** It says `GET /export` is not built
   yet; [backend/timeline-api/src/routes/export.rs](backend/timeline-api/src/routes/export.rs)
   exists and the plan records that route as committed. The template's resources for it are still
   missing, so the substance holds, but the comment reads as if the route itself were absent.

## Open items carried in the plans

Two defects found while checking Phase 3, both scheduled behind the phases, neither fixed:

- **Conversation order changes between backend restarts.** `list_for_user` in
  [backend/timeline-storage/src/memory/conversations.rs](backend/timeline-storage/src/memory/conversations.rs)
  walks a hash map and collects without sorting, and the walk order is not fixed between processes.
  The plan records observing three different orders from three runs of the same unmodified page. A
  user sees the conversation list and its colors reshuffle; a test that says "the first conversation"
  becomes order-dependent, and one did and had to be rewritten. The fix is known: sort by a stable
  key. [backend/timeline-api/src/routes/export.rs](backend/timeline-api/src/routes/export.rs)
  already sorts upload ids but never sorts the summaries.
- **Intermittent "Failed to fetch" from the page to the backend, not root-caused.** Several runs had
  the page report it could not reach the backend while the backend was answering `curl`. It predates
  the deletion, so it is not a regression. Serving over `http` rather than opening the file directly
  did not stop it; waiting for reachability before driving the browser did. Worked around, not
  explained — a user hitting it sees only "Is the backend running?" when it is.

Still open by design, each with a stated trigger:

- **Bedrock sign-off** (migration `C6`) — V3 needs one real captured call, so it needs a decision
  about briefly enabling model access on a real account.
- **Testing the real adapters without a container** (migration `C10`) — described as real, unstarted
  work. DynamoDB Local is a downloadable program needing a Java runtime that is not installed. For
  S3 there is no clean answer yet: the obvious candidate's license has changed to a commercial
  product, and the alternatives are unchecked.
- **The deduplication statistic in
  [timeline-project-decisions.md](timeline-project-decisions.md)** (migration `C8`) — the document
  says 31 of 4,482 messages were duplicates; measuring the same algorithm against the file actually
  supplied gave 6 of 4,457. Both reach the same final count, and the gap in raw messages equals the
  gap in dropped messages exactly, which fits the explanation that 25 duplicates had already been
  stripped before that file arrived. The plan marks this a strongly supported inference, not a
  confirmed fact, and leaves the decision to correct the line to the user.
- **The client-side parse still cannot show real progress** (frontend `C1`) — it is synchronous work
  over the whole export. The trigger is a measurement after Phase 3, and that measurement has not
  been taken.
- **Naming the two detection tiers** — tabled at the user's direction. Phase 4 changed *when* the
  non-generative pass runs and made it visible while running; no user-facing wording was invented.
  So the question that prompted Phase 4, whether users can tell this pass apart from the button that
  classifies with a model, is answered in the mechanism but not yet in the interface.
- **The defect in `CLAUDE.md`'s own wording rule**, found by the experiment: read literally, "use a
  word only if it appears in the user's own messages, in this file, or in the codebase" forbids
  ordinary connectives. The worked examples under it make the intent clear; the rule sentence does
  not.

One small inconsistency worth a glance: frontend `C8` is tagged `[OPEN]` on the question of whether
restoring a session on load should be announced, while the phase table one screen above records
"restore is announced, per your call." The tag looks stale rather than the work incomplete.
```
