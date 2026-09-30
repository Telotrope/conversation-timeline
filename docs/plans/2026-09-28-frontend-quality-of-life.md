# Frontend quality-of-life pass: launcher, test net, strip the page, make detection explicit, then polish

**Supersedes** the separate dev-launcher and upload-progress/drift plans (both folded in here and
deleted; git history keeps them). Their critique entries are carried forward with new numbers.

Six phases. Ordering reflects your decisions: the launcher goes first, and the e2e safety net goes
in before the big deletion. These are quality-of-life fixes to finish **before** returning to the
migration plan's server work (real S3/DynamoDB adapters, its C10).

| # | Phase | Touches | Status |
|---|---|---|---|
| 1 | Dev-server launcher + VS Code task | `scripts/`, `.vscode/` | **Done** |
| 2 | e2e coverage for calendar + analytics | `e2e/` | **Done** |
| 3 | Delete the page's detection logic and lexicons | `timeline.html`, `e2e/` | **Done** — verified output-neutral byte-for-byte |
| 4 | Detection runs only when asked, with visible progress | `timeline.html`, `backend/` | **Done**; user-facing naming of the two tiers still tabled |
| 5 | Upload progress bar | `timeline.html` | **Done** |
| 6 | Back/forward navigation + restore on load | `timeline.html` | **Done** — restore is announced, per your call |

---

## Phase 1: dev-server launcher and VS Code task

`scripts/dev-up.sh {backend|static}`. The two roles need different logic, because only one has
anything that can go stale.

**`backend`** — compiled code, so a running instance can be serving older code than what's on disk:
1. Run `cargo build -p timeline-api` unconditionally. This *is* the freshness check — cargo's
   incremental build already tracks the full dependency graph (including `Cargo.toml` bumps that
   touch no `.rs` file), which a hand-rolled mtime scan would get wrong. Sub-second when nothing
   changed.
2. Hash the resulting binary; compare against `.dev-state/timeline-api.hash`, written when this
   script last actually started an instance.
3. Check what's listening on `$PORT` (`lsof -ti tcp:$PORT`):
   - **Nothing listening** → start fresh, record the hash, `exec cargo run -p timeline-api`.
   - **Listening, hash matches, command line looks like our binary** → print `already up to date
     (pid N) — leaving it running`, exit 0. **No signal is ever sent.**
   - **Listening but hash differs, or something else is squatting** → print the occupant's PID and
     full command line, `SIGTERM`, wait 3s, `SIGKILL` if needed, then start fresh.

Why this matters more than saved seconds: `timeline-api` holds every upload and confirmed flag **in
memory only**, so an unnecessary restart silently deletes whatever you were looking at.

**`static`** — `python3 -m http.server` re-reads `timeline.html` from disk on every request, so a
running instance can never serve stale content. Liveness-only: if it's up and answering
`curl -sf .../timeline.html`, leave it; otherwise clear and start.

`scripts/dev-down.sh {backend|static|all}` — unconditional clearing, for manual use and the test's
teardown.

**VS Code tasks** (`.vscode/tasks.json`): one leaf task per role (`isBackground: true`, problem
matchers keyed to `listening on http://` and `Serving HTTP on`), plus a compound
`"Dev: Start local environment"` with `dependsOrder: parallel`, marked as the default build task so
"Run Build Task" triggers it — no extension needed, works in VS Code Web.
`"presentation": {"reveal": "silent"}` keeps a no-op "already up to date" run from stealing focus.

A live task pane is *sufficient* evidence the server is running but no longer *necessary*, since
most reruns find it current and exit without attaching. Liveness truth stays with `lsof`.

**Testing** (`scripts/test-dev-up.sh`, run manually — it manages real ports and processes):
1. *Stale occupant*: park a throwaway `python3 -m http.server 3000`, run the script, assert the real
   `timeline-api` replaced it.
2. *Already current*: note the PID, rerun with no source change, assert the **PID is unchanged** and
   **no `kill` was invoked at all** (run under a stubbed `kill` that records calls). This is the test
   that catches the data-loss bug your correction identified.
3. *Genuinely changed*: touch a file under `backend/timeline-core/src/`, rerun, assert the PID changes
   and the new process answers `/conversations`.
4. Same for the static server, minus case 3 (staleness doesn't apply).
5. Teardown via `dev-down.sh all` in a trap.

---

## Phase 2: e2e coverage for the views the deletion could break

Goes in **before** Phase 3, per your call. The current suite covers upload → render → flag →
reload; it does not touch the calendar or the analytics views, which is precisely where a large
deletion could break something silently.

### What the audit found

A full coverage audit was run. Rust is in good shape: **173 tests pass, 77.57% line coverage
measured fresh with `cargo llvm-cov`**, and **every route has at least one test** — the gaps there
are inside handlers, not whole endpoints. The frontend is where the holes are.

Ordered by risk, the things **no test of any kind touches**:

| Gap | Why it matters |
|---|---|
| **All five analytics views** — `computeFrictionAnalysis`, `computeTrendAnalysis`, `computeLengthAnalysis`, `computeTimeOfDayAnalysis`, `computeIdleGapAnalysis`, their five renderers, all three chart renderers, `pearsonR` (~370 lines) | Largest untested block in the page. `runAnalysis` only fires on a `data-analysis` click and the suite never opens that tab. Wrong-number bugs here are silent. |
| **`exportAnnotatedConversations`** — "Download annotated conversations.json" | Never clicked. This is the data-*out* path; a bug here loses annotation work. |
| **`selectConversation` → `renderMarkdownLite` → `escapeHtml`** | The suite asserts on `#convItems` text but never opens a conversation. `escapeHtml` being untested is the one gap with a correctness/injection flavor. |
| **`resolveApiBase`'s `?api_base=` branch** | Only the default fallback is covered — the query-param and localStorage branches, i.e. exactly the mechanism the remote/Tailscale setup depends on, are untested. |
| **Review search, `#reviewFilter`, pagination, the three global toggles** | Render and the Approve button are covered; none of the controls are. |
| **Calendar interactions** — day clicks, `jumpToReviewDay`, `shiftReviewDay` | Calendar *rendering* turns out to be incidentally smoke-covered (the load path always calls `renderCalendar`, and two tests assert zero console errors). The interactions are not. |
| **Error paths** — `describeFailure`, the backend-unreachable `TypeError` hint | Untested. |
| **`classifyWithAI`** | Untested, and by its own comment it is artifact-only and slated for replacement by V3's Bedrock path. Possibly already dead in this deployment — worth deciding whether to delete rather than test. |

### New tests for this phase

In [e2e/](../../e2e/), against the same real backend the existing suite uses — prioritizing the
table above, and specifically the things Phase 3's deletion could break:
- **Each of the five analytics views** renders non-empty content with no console errors.
- **Calendar** renders day cells with session blocks for the fixture's real dates, and a day click
  navigates as expected.
- **Opening a conversation** renders its transcript (covers the markdown/escaping path).
- **Review search, filter, and pagination** return plausible subsets.
- **The annotated-export download** produces a file containing the flags.
- **A message the backend flagged renders as flagged** — the assertion that proves flags come from
  the backend rather than a client-side pass, which is what Phase 3 removes.

### Structural gaps the audit surfaced, for the record

These are outside this phase's scope but shouldn't be lost — see C11 and C12:
- **`main.rs` is 0% covered**, including the branch deciding that `/_dev/*` routes are absent from
  the Lambda build.
- **There is no CI of any kind** (no `.github/`), and the e2e suite is wired into nothing — it runs
  only when someone remembers to run it.
- **`tests/test_no_unhandled_exceptions.py`, referenced in CLAUDE.md, does not exist in this repo.**
  No silent-swallow check runs on either Rust or JavaScript.
- **The file-size ratchet exists** ([backend/timeline-core/tests/file_sizes.rs](../../backend/timeline-core/tests/file_sizes.rs),
  passing, empty allowlist) **but only scans `.rs` files under `backend/`** — `timeline.html` is
  entirely outside its reach.

---

## Phase 3: delete the page's own detection logic and its two lexicons

### What's actually in the file

[timeline.html](../../timeline.html) is 752,370 bytes / 66,839 lines. Measured:

| Block | Location | Bytes | Share |
|---|---|---|---|
| `DICTIONARY_WORDS_RAW` — embedded English word list | [timeline.html:960-64834](../../timeline.html#L960) | 594,666 | 79% |
| `AFINN` — embedded sentiment lexicon | [timeline.html:64864](../../timeline.html#L64864) | 46,252 | 6% |
| Everything else — UI, rendering, calendar, analytics, review table | — | ~111,000 | 15% |

**85% of the file is lexicon data feeding detection the backend now performs.** Deleting it leaves
roughly 112KB / ~2,965 lines — a file you can read a diff against.

### Why this is safe — verified, not assumed

1. **The detection functions have exactly two call sites**, both inside
   `parseUploadedConversations`: the drift comparison at
   [timeline.html:65011-65013](../../timeline.html#L65011-L65013) and the "refresh detection" path
   at [timeline.html:65019-65021](../../timeline.html#L65019-L65021). Nothing else calls them —
   rendering, the review table, the calendar, and analytics all read the already-computed
   `default_caps`/`default_critical`/`default_angry` fields.
2. **The backend computes auto flags for every human message.** `process_upload` in
   [backend/timeline-api/src/processing.rs](../../backend/timeline-api/src/processing.rs) skips
   non-human messages, then calls `set_auto_flags` unconditionally for the rest.
3. **The export embeds those flags per message** —
   [backend/timeline-api/src/routes/export.rs](../../backend/timeline-api/src/routes/export.rs)
   writes `_claude_timeline_auto` and `_claude_timeline_user` in the shape the page already reads at
   [timeline.html:64991-64992](../../timeline.html#L64991-L64992).
4. **The page's own deduplication is already unreachable.** The export serializes
   `{"conversations": [...]}`, sending the page down the `alreadyProcessed: true` branch at
   [timeline.html:64961-64962](../../timeline.html#L64961-L64962), which does not dedup. Client
   dedup only runs on a bare top-level array, which the backend never produces. Cross-checked in
   Rust: `unwrap_uploaded_value` dedups the bare-array case only
   ([format.rs:70](../../backend/timeline-core/src/format.rs#L70)), and both `process_upload` and
   `export` call it on the same raw text, so they agree.

Note fact 2 is what Phase 4 would change — see the interaction note there.

### What gets deleted

- `DICTIONARY_WORDS_RAW`, `DICTIONARY`, `CAPS_EXCLUDE`, `AFINN`, `ANGER_LEXICON_RE`, the sentiment
  scorer, `findEmphasisCapsWords`, `detectCritical`, `detectAngry`.
- `dedupChatMessages` / `dedupConversations` — unreachable per fact 4.
- The drift modal ([timeline.html:901-909](../../timeline.html#L901-L909)), `showDriftModal`, and
  the whole `driftCount` mechanism. **You approved this.** With no second implementation in the
  browser there is nothing to compare, so the feature dies rather than becoming lazy.
- **The e2e drift block** at [e2e/upload-flow.spec.js:63-66](../../e2e/upload-flow.spec.js#L63-L66)
  and its 13-line explanatory comment. To be precise about what that costs: those four lines are a
  conditional modal dismissal inside the shared `loadFixtureAndWaitForRender` helper, not a test of
  their own — no test disappears, and all three existing assertions still run through the same real
  backend.
- `parseUploadedConversations` loses its `runAutoDetect` / `refreshAutoDetect` parameters and its
  `driftCount` return field.
- The "Refresh automatic tags saved in this file" checkbox
  ([timeline.html:768-780](../../timeline.html#L768-L780)) — re-uploading already re-runs the
  backend's detection with current code, which is what the checkbox promised.

### Stale user-facing copy to fix in the same pass

- [timeline.html:896](../../timeline.html#L896) tells the user "Your conversation export is read
  locally in this browser tab and is never uploaded anywhere." That is now **false**. Must be
  corrected regardless of the rest of this phase.
- `window.storage` residue ([timeline.html:65385](../../timeline.html#L65385),
  [:65407](../../timeline.html#L65407)) — an auto-flag cache still written to the Claude-artifact
  storage API though flags now live in the backend. Sweep it and the stale comments at
  [:65378-65394](../../timeline.html#L65378).

---

## Phase 4: make detection an explicit step — answering "why does the backend always detect?"

### Why it currently does

Not an oversight, but a decision recorded in the migration plan: line 32-33 of
[docs/plans/2026-09-09-rust-aws-backend-migration.md](2026-09-09-rust-aws-backend-migration.md)
specifies a **free heuristic tier** (dictionary caps + keyword/sentiment criticism-anger, "~$0
marginal cost") that is "always available," against **one Bedrock-quality classification pass per
$5** (V3/V4, unbuilt).

So the intent was "free tier, always available." What got built conflates that with "always already
computed, at upload, whether or not anyone asked." Those are different claims, and you're right that
the second one contradicts what you asked for.

**One thing I should not dress up:** the CPU argument here is weak. Server-side this is Rust doing
dictionary lookups and sentiment scoring over a few thousand messages. I have *not* measured it, and
I'm not going to claim it's slow — my expectation is tens of milliseconds, which is nothing like the
client-side case, where the same work ran in JavaScript on the UI thread and was thrown away. The
real arguments for changing it are **control** (don't compute what wasn't asked for) and **clarity**
(below), not speed.

### Decided: it runs only when asked, and the user can watch it run

Two requirements, both from the user, neither optional:

1. **The non-generative detection pass runs only on an explicit user action** — a checked box or a
   pressed button — and never otherwise. Not on upload, not implicitly, not "because it's cheap."
2. **While it runs, the user can see that it is running**, as progress, not as a frozen screen. It is
   a pass over every speech act in the export; it is not instantaneous and must not be presented as
   though it were.

**On requirement 2 and what this plan is allowed to claim about duration:** nothing here has been
measured. The pass is work proportional to the number of speech acts, and this plan does not assert
it is fast, cheap, or instant — earlier drafts did, with no measurement behind the word, which is
exactly the kind of claim this project's conventions forbid. The progress display exists because the
duration is *unknown and non-trivial*, not as decoration over something already known to be quick.

### Design

**Trigger.** Detection moves out of `process_upload` into its own user-triggered route. Upload does
upload: parse, dedup, store, return. The timeline itself — calendar, sessions, conversation list —
needs no flags at all, so the first render is complete and correct rather than empty.

The trigger is a checkbox on the load screen, **unchecked by default**, meaning "also run detection
once the upload is in." Chosen over a Review-tab button for one concrete reason: it composes directly
with Phase 5's load progress bar, so detection becomes another labelled phase of a bar the user is
already watching, which is what requirement 2 asks for. A separate button that can run it later
(or re-run it) is a reasonable addition but is **not** specified here — see the note on tabled work
below.

**Progress.** A single request that returns when the whole pass is done cannot report progress. The
client drives the loop instead, in batches:

- The page requests detection for a batch of messages, gets the resulting flags, advances the bar,
  and repeats until done.
- This reuses the idiom already in the page: `classifyWithAI` at
  [timeline.html:65605-65650](../../timeline.html#L65605) already batches its work
  (`CLASSIFY_BATCH_SIZE`) and drives `.progress-track` / `.progress-fill` / `.progress-label` per
  batch. Same CSS, same shape of loop, no new streaming or background-job machinery — which per this
  repo's reuse-order rule beats adding server-sent events or a polling status endpoint.
- Because progress is measured in batches completed out of batches total, the bar is genuinely
  determinate here, unlike the phases in Phase 5 that have nothing observable to report.

**Proposed route shape** (a proposal, not a settled interface): `POST /detect` taking a conversation
id and a list of message uuids, returning the computed flags for exactly those messages and writing
them through the existing `AutoFlagWriter`. Client-enumerated ids rather than server-side offsets,
because that keeps the batching explicit and testable from the outside.

**Cost, stated plainly:** this is backend work. It is a small piece — lifting an existing loop out of
`process_upload` into a route handler, no new adapters, no AWS — but it is not zero, it needs its own
tests, and it means the raw upload is re-read at detection time rather than riding along with a pass
already in progress.

### What this phase does NOT decide

The user has **tabled** the question of how the two tiers are named and explained to users —
non-generative versus generative emotion detection — and will design that separately. So this phase
deliberately does not invent user-facing labels, does not pair the two triggers in the interface, and
does not touch the existing "auto"/"AI" source markers at
[timeline.html:66198](../../timeline.html#L66198) and [:66215](../../timeline.html#L66215). It changes
*when* the pass runs and *whether the user can see it running*. Nothing else.

### Interaction with Phase 3

Fact 2 in Phase 3 ("the backend computes auto flags for every human message") is exactly what this
phase changes, so the two must not land together unverified. Phase 3's deletion is correct either way
— the page has no business running detection itself in either design — but:

- **Phase 3 alone should be output-neutral**: the rendered page must look identical before and after.
  This is checkable rather than hopeful, because the flags being rendered already come from the
  backend today — when "refresh detection" is unchecked, the defaults are read straight from the
  stored values at [timeline.html:65001-65005](../../timeline.html#L65001-L65005), and the page's own
  detectors only feed the drift count. Deleting code that wasn't feeding the render should change
  nothing on screen.
- **Phase 4 deliberately changes output**: with detection no longer running at upload, a freshly
  uploaded export renders with no flags until the user asks for them. That is an intended change and
  must be verified as such, not mistaken for a Phase 3 regression.

Hence Phase 3 first, verified output-neutral, then Phase 4 with its own expected-difference baseline.

---

## Phase 5: real progress for the upload, honest labels elsewhere

`handleLoadClick` ([timeline.html:65115-65204](../../timeline.html#L65115-L65204)) runs six phases
that differ in whether progress is observable at all. The design says so rather than inventing
numbers:

| Phase | Observable? | Shown |
|---|---|---|
| `_dev/login`, `POST /uploads` | No — single fast requests | Label only |
| `PUT {upload_url}` (the bytes) | **Yes** | **Bar + % + ETA** |
| Server-side tail of that same PUT | No | Bar indeterminate, `Finishing up on the server…` |
| `GET /export` | No | Label only |
| `GET {export_url}` (download) | **Yes, if `Content-Length` is set** | Bar + %, else running byte count |
| Client-side parse | Not without restructuring | Label only — see C1 |

That fourth row matters: per [timeline.html:65142-65146](../../timeline.html#L65142-L65146), the
local-dev `PUT` handler processes the upload *synchronously before responding*, so byte progress
reaching 100% doesn't mean the wait is over. A full bar with nothing happening looks frozen, so the
label changes to say what's going on.

- **Upload progress needs `XMLHttpRequest`** — `fetch` has no upload-progress facility. Only the
  `PUT` at [timeline.html:65139](../../timeline.html#L65139) changes; everything else stays on
  `fetch`. A built-in browser API beats adding a library, per the reuse-order rule.
- **Download progress** via `fetch` + `response.body.getReader()` against `Content-Length`; with no
  such header, show transferred bytes, never a fabricated percentage.
- **ETA** from a rolling ~3-second window, suppressed until two samples exist, rendered as
  `about 20 seconds left` — never `18.4s`.
- **Reuses the existing progress idiom** — `.progress-track` / `.progress-fill` / `.progress-label`
  at [timeline.html:534-553](../../timeline.html#L534-L553), already used by the AI-classify bar and
  analytics. No new CSS; `.is-error` already exists.

---

## Phase 6: Back means "back one step", and leaving the page stops being expensive

### Hash routing

Nothing in the page touches the History API today — no `pushState`, `hashchange`, or `popstate` —
so tab changes, conversation selection ([timeline.html:65906](../../timeline.html#L65906)), and
analytics selection are invisible to the browser, and Back exits the page entirely.

Write that state to `location.hash` (`#calendar`, `#conversations/42`, `#analytics/friction`) and
restore on `hashchange`. Real Back/Forward, plus bookmarkable deep links.

**Hash, not `pushState`, because the page is still opened as a `file://` URL** — the e2e suite does
exactly that at [e2e/upload-flow.spec.js:13](../../e2e/upload-flow.spec.js#L13) — and `pushState` is
restricted for file URLs in some browsers, while hash works everywhere.

Scope: those three axes. Review search/pagination stay out of the hash (they'd churn history on every
keystroke).

### Restore on load, using the affordance that already exists

- On startup, if a dev login name is remembered in `localStorage`, call `GET /export` and render
  directly — no file picking, no re-upload.
- If that returns nothing, or the server was restarted (in-memory storage), fall back to the load
  screen as today.
- **"Load a different file…"** ([timeline.html:797](../../timeline.html#L797), handler at
  [:65237-65243](../../timeline.html#L65237)) already does the right thing; it needs rewiring only to
  also clear the remembered session so the next startup doesn't silently restore the old export.
- The dev login name must be persisted to `localStorage` on successful login; it currently isn't.

---

## Self-critique log

### C1 [OPEN]: the client-side parse phase still can't show real progress
`parseUploadedConversations` is synchronous main-thread work over the whole export, so even an
indeterminate label can't animate during it. Real progress needs chunked parsing or a Web Worker.
**Mitigation:** honest label rather than a fake bar; Phase 3 removes the largest avoidable part of
the cost. **Open:** revisit if it's still a visible freeze after Phase 3 — that measurement is the
trigger and can't be taken until then.

### C2 [RESOLVED]: `fetch` can't report upload progress
**Resolution:** the single `PUT` carrying the body moves to `XMLHttpRequest`; everything else stays
on `fetch` — [§Phase 5](#phase-5-real-progress-for-the-upload-honest-labels-elsewhere).

### C3 [RESOLVED]: a naive ETA is worse than none
**Resolution:** rolling ~3-second window, suppressed until two samples, rounded hedged language —
[§Phase 5](#phase-5-real-progress-for-the-upload-honest-labels-elsewhere).

### C4 [RESOLVED]: deferring the drift check was the wrong answer
Original position was to keep the check as a JavaScript-vs-Rust consistency signal. That doesn't
survive Phase 3 — keeping a second detection implementation alive so it can disagree with the real
one inverts the point of the migration. Lost: the live divergence signal on real data. Covering it
instead: `timeline-core`'s own tests, plus the Phase 2 assertion that backend-produced flags render.
**Resolution:** deleted, not deferred — [§Phase 3](#what-gets-deleted). Approved by user.

### C5 [RESOLVED]: unconditionally killing the running backend is destructive
Raised by you. `timeline-api` keeps all state in memory, so "always kill, then start fresh" discarded
the session on every rerun — including reruns that only meant to *check*. **Resolution:** build-hash
comparison, static server exempt from staleness entirely — [§Phase 1](#phase-1-dev-server-launcher-and-vs-code-task),
with both branches as explicit test cases.

### C6 [RESOLVED]: killing "whatever is on the port" could hit an unrelated process
**Resolution:** scoped to two project-specific ports, never a system-wide scan; the occupant's PID and
full command line are printed before any signal — and after C5, a signal is only sent when the
occupant is actually stale or actually wrong.

### C7 [RESOLVED]: `pushState` would break the `file://` use case
Standard SPA routing reaches for `pushState`, but the e2e suite loads the page as `file://`, where
`pushState` is restricted in some browsers — history built that way would pass manual testing and
fail the test meant to protect it. **Resolution:** `location.hash` — [§Phase 6](#hash-routing).

### C8 [OPEN]: restore-on-startup changes what the page does when you open it
The page would no longer always open on the load screen; a stale restored session could confuse.
**Mitigation:** "Load a different file…" is the escape hatch and already works, needing only to clear
the remembered session. **Open:** silent restore vs. an announced one ("restored your last export").
Trigger: your preference — cheap to change either way.

### C9 [RESOLVED]: Phase 3 is a large deletion with a thin automated net
Cutting 85% of a file is where things break quietly, and the suite didn't cover the calendar or
analytics. **Resolution:** you approved adding that coverage first — it's now
[§Phase 2](#phase-2-e2e-coverage-for-the-views-the-deletion-could-break), sequenced before the
deletion.

### C10 [RESOLVED]: "always available" was silently implemented as "always already computed"
The migration plan promised a free tier that's *available*; the code computes it unconditionally at
upload, which is not the same claim and contradicts what the user asked for.
**Resolution:** detection moves behind an explicit trigger and reports progress while it runs, per
[§Phase 4](#decided-it-runs-only-when-asked-and-the-user-can-watch-it-run). The `detect: false`
parameter alternative was rejected: it would stop the unwanted computation but leaves the user with no
indication that a pass over every speech act is happening when they do ask for it.

### C13 [RESOLVED]: this plan twice asserted durations it had never measured
"Free, instant" was written about the non-generative pass in conversation, one message after the same
plan had explicitly said it hadn't been measured and wouldn't be characterized. The pass is work
proportional to the number of speech acts in the export; no timing for it exists anywhere in this
repo.
**Resolution:** every duration claim about it is removed. The progress display in
[§Phase 4](#design) is justified by the duration being *unknown*, not by any measurement. If a timing
claim is ever wanted, it needs a measurement first.

### C11 [RESOLVED]: nothing verifies that the `_dev` routes stay out of the Lambda build
`main.rs` measures 0% coverage — all six functions unexecuted, because tests build routers directly
via `build_router` and bypass `main` entirely. Its module doc states the Lambda branch is handed
`build_router` alone, so `POST /_dev/login` and the local-storage routes are structurally absent
from anything deployable. Reading the source, that claim looks correct — but **no test asserts it**,
and it is the one property in this codebase where being wrong would mean shipping an unauthenticated
token-minting endpoint to production.
**Resolution:** [backend/timeline-api/tests/lambda_router.rs](../../backend/timeline-api/tests/lambda_router.rs)
builds exactly what the Lambda branch builds and asserts every `/_dev/*` path 404s — and, because
that alone would also pass on an empty router, asserts the real routes are present and merely
unauthorized. Brought forward from "before deployment" by the arrival of `POST /_dev/reset`, which
erases everything: a route like that must not be one wiring mistake away from production.

### C12 [RESOLVED]: "run all tests" had been narrowed to exclude the slow suites
Framed originally as a missing-CI problem. It isn't: the standing instruction is to run all tests
before every commit, and the defect was that `cargo test --workspace` excludes the e2e suite and the
launcher test, so "all" had quietly come to mean "the fast ones." That is a subset invented to avoid
work, not a property of the project.
**Resolution:** one command runs every suite — the Rust workspace, the Playwright e2e suite, and the
launcher test — and that command is what "run all tests" means. It also pins the Node version the e2e
suite requires (the default `node` here is 18; the suite needs 20), so an environment mismatch fails
loudly instead of looking like a broken test.
**Also in scope, per the user's direction:** a silent-exception-swallow check covering both the Rust
and the JavaScript. CLAUDE.md describes one at `tests/test_no_unhandled_exceptions.py`; no such file,
and no Python at all, exists in this repo, so it has to be written rather than wired up.
**Remaining, separately:** the file-size ratchet only scans `.rs` under `backend/`, so `timeline.html`
is invisible to it. Worth a JavaScript arm once Phase 3 makes the file small enough for a line-count
rule to mean anything.

## Open questions for review

1. **`scripts/` + bash** for Phase 1, versus a `justfile`/`Makefile`. Bash chosen only because the
   README already documents raw shell commands.
2. **C8**: silent restore, or announced?

## Known defects — fix before shipping, lower priority than the phases

Both were found while verifying Phase 3, neither is caused by it, and the user has scheduled them
behind the quality-of-life phases.

1. **Conversation order is non-deterministic across backend restarts.** `list_for_user` in
   [backend/timeline-storage/src/memory/conversations.rs](../../backend/timeline-storage/src/memory/conversations.rs)
   iterates a `HashMap` and collects without sorting, and Rust seeds its hasher randomly per
   process. Observed directly: three runs of the *same* unmodified page produced three different
   conversation orders. User-visible as the conversation list and the per-conversation colors
   reshuffling between sessions, and it makes any test that says "the first conversation"
   order-dependent — one did, and had to be rewritten. Fix: sort the returned summaries by a stable
   key. Note `export.rs` already sorts `upload_ids` but never sorts `summaries`.
2. **Intermittent "Failed to fetch" from the page to the backend, cause unknown.** Several capture
   runs failed with the page reporting it could not reach `http://127.0.0.1:3000` while the backend
   was demonstrably listening and answering `curl`. It affected the pre-deletion build equally, so
   it is not a regression. Serving over `http` instead of `file://` did not eliminate it; explicitly
   waiting for reachability before driving the browser did. **Not root-caused** — worked around, not
   explained. Worth isolating before shipping, since a user hitting this sees only "Is the backend
   running?" when it is.
3. **Friction ranking can only be sorted by % flagged.** Added 2026-09-30 at the user's request.
   It should sort by any column, and needs a new start-date column (a session's first message; a
   conversation's earliest session). Today `computeFrictionAnalysis` in
   [frontend/core/analyses.js](../../frontend/core/analyses.js) sorts once by percentage, and
   `renderFrictionResult` in [frontend/ui/views/analytics.js](../../frontend/ui/views/analytics.js)
   draws fixed headers.

## Tabled at the user's direction

- **How the two detection tiers are named and explained to users** — non-generative versus generative
  emotion detection. The user designed this and will produce the design; this plan touches only *when*
  the non-generative pass runs and whether its progress is visible. No user-facing labels are invented
  here.
