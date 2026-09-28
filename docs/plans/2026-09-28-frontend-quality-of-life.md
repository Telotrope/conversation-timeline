# Frontend quality-of-life pass: launcher, test net, strip the page, make detection explicit, then polish

**Supersedes** the separate dev-launcher and upload-progress/drift plans (both folded in here and
deleted; git history keeps them). Their critique entries are carried forward with new numbers.

Six phases. Ordering reflects your decisions: the launcher goes first, and the e2e safety net goes
in before the big deletion. These are quality-of-life fixes to finish **before** returning to the
migration plan's server work (real S3/DynamoDB adapters, its C10).

| # | Phase | Touches | Status |
|---|---|---|---|
| 1 | Dev-server launcher + VS Code task | `scripts/`, `.vscode/` | **Approved to build** |
| 2 | e2e coverage for calendar + analytics | `e2e/` | **Approved** |
| 3 | Delete the page's detection logic and lexicons | `timeline.html`, `e2e/` | **Approved**, incl. the e2e drift block |
| 4 | Make detection an explicit, user-triggered step | `timeline.html`, `backend/` | **Open — needs your decision, see §Phase 4** |
| 5 | Upload progress bar | `timeline.html` | Approved in substance |
| 6 | Back/forward navigation + restore on load | `timeline.html` | Approved in substance |

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

### The confusion you predicted is real

The review table already distinguishes the two sources — "auto" vs "AI" with tooltips at
[timeline.html:66198](../../timeline.html#L66198) and [:66215](../../timeline.html#L66215), driven by
`auto_source` being `'heuristic'` or `'llm'`. But nothing tells the user *how they relate*: the
heuristic is configured by a checkbox on the load screen, while the AI pass is a button inside the
Review tab, with no shared framing. They're two tiers of the same operation presented as unrelated
features in different places.

### Recommendation: make both tiers explicit, side by side

Move detection out of `process_upload` into its own user-triggered route, and put its button
directly next to "Classify with AI" in the Review tab:

- **Upload does upload.** Parse, dedup, store, return. The timeline itself — calendar, sessions,
  conversation list — needs no flags at all, so this first render is complete and correct, not empty.
- **The Review tab offers two clearly-paired choices**, e.g. "Scan with keywords (free, instant)" and
  "Classify with AI (more accurate)" — same place, same shape, obvious relationship, with the
  existing "auto"/"AI" source labels then meaning something.
- **The load-screen "Automatically detect flags" checkbox is deleted**, since the choice now lives
  where the results appear.

Cost of this option, stated plainly: it's backend work, which you wanted to defer. It's a small piece
— lifting an existing loop out of `process_upload` into a new route handler, no new adapters, no
AWS — but it is not zero, and it needs its own tests. It also means the raw upload gets re-read and
re-parsed at detection time rather than riding along with the pass already in progress.

**Cheaper alternative if you'd rather not touch the backend much:** keep detection at upload but
have the page send an explicit flag (`POST /uploads` with `detect: false`), so nothing is computed
unless asked. One boolean, no new route, no re-parsing. It satisfies "don't compute by default" but
leaves the two tiers as unrelated-looking features in different parts of the UI, so it does not fix
the confusion you flagged.

### Interaction with Phase 3

Fact 2 above ("the backend computes auto flags for every human message") is exactly what this phase
changes. Phase 3's deletion stays correct either way — the page has no business running detection
itself in either design — but the story changes from "flags always arrive with the export" to "flags
arrive once you've asked for them." Messages simply render unflagged until then. If both phases land,
Phase 3 should go first so the deletion is reviewed against today's behavior rather than two moving
parts at once.

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

### C10 [OPEN]: "always available" was silently implemented as "always already computed"
The migration plan promised a free tier that's *available*; the code computes it unconditionally at
upload. Phase 4 proposes separating those, but the choice between the full fix (own route, paired UI)
and the cheap fix (a `detect: false` parameter) is yours, and the full fix is backend work you'd
wanted to defer. **Open:** your decision on which option, per
[§Phase 4](#recommendation-make-both-tiers-explicit-side-by-side).

### C11 [OPEN]: nothing verifies that the `_dev` routes stay out of the Lambda build
`main.rs` measures 0% coverage — all six functions unexecuted, because tests build routers directly
via `build_router` and bypass `main` entirely. Its module doc states the Lambda branch is handed
`build_router` alone, so `POST /_dev/login` and the local-storage routes are structurally absent
from anything deployable. Reading the source, that claim looks correct — but **no test asserts it**,
and it is the one property in this codebase where being wrong would mean shipping an unauthenticated
token-minting endpoint to production.
**Open:** add a test that builds the Lambda-path router and asserts every `/_dev/*` path returns
404. Cheap to write. Trigger: before any real deployment — this must not be outstanding when V2
deploys.

### C12 [OPEN]: the test suites only run when someone remembers
No CI exists. The e2e suite is manual by design and wired into nothing, the launcher test is manual,
and the file-size ratchet only covers `.rs` files under `backend/` — `timeline.html` is outside it.
Separately, CLAUDE.md refers to a silent-exception-swallow check (`tests/test_no_unhandled_exceptions.py`)
that does not exist in this repo at all.
**Mitigation in plan:** none — this is out of scope for a quality-of-life pass.
**Open:** whether to add CI, and whether the ratchet should grow a JavaScript arm once Phase 3
shrinks `timeline.html` to a reviewable size. Trigger: after Phase 3, when the file is small enough
for a line-count rule to mean something.

## Open questions for review

1. **Phase 4: which option** — the full separation (own route + paired buttons in the Review tab,
   which also fixes the heuristic-vs-AI confusion), or the cheap `detect: false` parameter (satisfies
   "don't compute by default," leaves the UI story unfixed)?
2. **`scripts/` + bash** for Phase 1, versus a `justfile`/`Makefile`. Bash chosen only because the
   README already documents raw shell commands. *(Proceeding with bash unless you say otherwise.)*
3. **C8**: silent restore, or announced?
