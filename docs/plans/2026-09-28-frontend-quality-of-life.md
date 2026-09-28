# Frontend quality-of-life pass: strip the page down, then fix the load flow, navigation, and dev launcher

**Supersedes** [2026-09-28-dev-server-launcher.md](2026-09-28-dev-server-launcher.md) and
[2026-09-28-upload-progress-and-lazy-drift-detection.md](2026-09-28-upload-progress-and-lazy-drift-detection.md),
both of which are folded in here and deleted when this plan is committed (git history keeps them).
Their critique entries are carried forward below with new numbers, noted per entry.

Four phases, ordered so that the first one makes every later diff readable. These are quality-of-life
fixes to finish **before** returning to server work (real S3/DynamoDB adapters, C10 in the migration
plan).

---

## Phase 1: delete the page's own detection logic and its two lexicons

### What's actually in the file

[timeline.html](../../timeline.html) is 752,370 bytes / 66,839 lines. Measured:

| Block | Location | Bytes | Share |
|---|---|---|---|
| `DICTIONARY_WORDS_RAW` — embedded English word list | [timeline.html:960-64834](../../timeline.html#L960) | 594,666 | 79% |
| `AFINN` — embedded sentiment lexicon | [timeline.html:64864](../../timeline.html#L64864) | 46,252 | 6% |
| Everything else — UI, rendering, calendar, analytics, review table | — | ~111,000 | 15% |

**85% of the file is lexicon data that exists solely to power detection the backend now performs.**
Deleting it leaves roughly 112KB / ~2,965 lines — a file you can actually scroll through and review
a diff against.

### Why this is safe to delete — verified, not assumed

Four things were checked in source before proposing this:

1. **The detection functions have exactly two call sites**, both inside
   `parseUploadedConversations`: the drift comparison at
   [timeline.html:65011-65013](../../timeline.html#L65011-L65013), and the "refresh detection" path
   at [timeline.html:65019-65021](../../timeline.html#L65019-L65021). `findEmphasisCapsWords`,
   `detectCritical`, and `detectAngry` are called nowhere else in the page. Rendering, the review
   table, the calendar, and analytics all read the already-computed `default_caps`/`default_critical`/
   `default_angry` fields, never the detectors.
2. **The backend computes auto flags for every human message.** `process_upload` in
   [backend/timeline-api/src/processing.rs](../../backend/timeline-api/src/processing.rs) skips
   non-human messages and then calls `set_auto_flags` unconditionally for the rest — there is no
   "only if flagged" filter. So every human message has a stored flag record.
3. **The export embeds those flags per message.**
   [backend/timeline-api/src/routes/export.rs](../../backend/timeline-api/src/routes/export.rs)
   writes `_claude_timeline_auto` (with `"source": "heuristic"`) and `_claude_timeline_user` onto
   each human message. That's the same shape the page already reads at
   [timeline.html:64991-64992](../../timeline.html#L64991-L64992).
4. **The page's own deduplication is already unreachable.** The export serializes
   `{"conversations": [...]}`, which sends the page down the `alreadyProcessed: true` branch at
   [timeline.html:64961-64962](../../timeline.html#L64961-L64962) — the branch that does *not*
   dedup. Client dedup only runs on a bare top-level array, which the backend never produces.
   Cross-checked on the Rust side: `unwrap_uploaded_value` dedups the bare-array case only
   ([backend/timeline-core/src/format.rs:70](../../backend/timeline-core/src/format.rs#L70)), and
   both `process_upload` and `export` call it on the same raw upload text, so the two agree.

One edge case, stated rather than glossed: `export` only embeds flags when `flags_reader.get(...)`
returns `Some`. Fact 2 makes that true for any upload that completed processing; a conversation
summary left behind by an upload that failed partway could in principle lack records, and those
messages would render unflagged instead of freshly detected in the browser. That is the correct
behavior anyway — the backend is the source of truth — but it is a behavior change, not a no-op.

### What gets deleted

- `DICTIONARY_WORDS_RAW`, `DICTIONARY`, `CAPS_EXCLUDE`, `AFINN`, `ANGER_LEXICON_RE`, the sentiment
  scorer, `findEmphasisCapsWords`, `detectCritical`, `detectAngry`.
- `dedupChatMessages` / `dedupConversations` ([timeline.html:64910](../../timeline.html#L64910),
  [:64946](../../timeline.html#L64946)) — unreachable per fact 4.
- The drift modal ([timeline.html:901-909](../../timeline.html#L901-L909)), `showDriftModal`, and
  the whole `driftCount` mechanism. **This supersedes the earlier "make the drift check lazy" plan**
  (see C4): with no second implementation in the browser, there is nothing left to compare against,
  so the feature is deleted rather than deferred. You asked not to pay for that computation unless
  you click the button; this goes further — the computation ceases to exist.
- The "Refresh automatic tags saved in this file" checkbox and its help text
  ([timeline.html:768-780](../../timeline.html#L768-L780)). Re-uploading the file already re-runs
  the backend's detection with current code, which is exactly what the checkbox promised.

### What has to be rewired, not just cut

- **`parseUploadedConversations` loses two of its three parameters.** With no client detection and
  no drift count, it takes the export text and returns parsed structures — the `runAutoDetect` /
  `refreshAutoDetect` arguments and the `driftCount` return field all go.
- **The "Automatically detect flags" checkbox** ([timeline.html:755-766](../../timeline.html#L755-L766))
  currently gates client-side detection. The backend always detects; the page can't turn that off.
  Proposal: repurpose it as a *display* toggle (hide automatic tags, show only your own) — which the
  page already has at [timeline.html:801](../../timeline.html#L801) as "Show automatic tags", making
  the load-screen checkbox redundant. **Recommend deleting it** and letting the existing global
  toggle cover it. Flagged as an open question since it changes the load screen's appearance.
- **The e2e test must change.** [e2e/upload-flow.spec.js:63-66](../../e2e/upload-flow.spec.js#L63-L66)
  clicks `#driftKeepSaved` when the modal appears, and its 13-line comment explains the AFINN/VADER
  threshold divergence that caused it. With the modal gone that block is dead and must be removed,
  along with the comment. Per this repo's "never modify a committed test without approval" rule this
  needs your explicit sign-off — **it is not covered by the general approval of this plan.** The
  three assertions themselves are unaffected and all still pass through the same real backend.

### Stale user-facing copy to fix in the same pass

- [timeline.html:896](../../timeline.html#L896) tells the user "Your conversation export is read
  locally in this browser tab and is never uploaded anywhere." That is now **false** — the page
  uploads to `timeline-api`. Must be corrected regardless of the rest of this phase.
- `window.storage` residue ([timeline.html:65385](../../timeline.html#L65385),
  [:65407](../../timeline.html#L65407)) — an auto-flag cache still written to the Claude-artifact
  storage API even though flags now live in the backend. Sweep it, along with the comments at
  [:65378-65394](../../timeline.html#L65378) that describe a retired mechanism.

### Testing

The existing e2e suite is the regression net and it already covers the right things: a real upload
renders real conversation content, a >2MB file still uploads, and a flag edit survives a reload. All
three must pass unchanged (modulo the drift-modal block above). Additionally: assert in the test
that a message the backend flagged renders as flagged, which is what proves flags are coming from
the backend rather than from a client-side pass that no longer exists.

---

## Phase 2: real progress for the upload, honest labels elsewhere

Carried forward from the superseded progress plan, unchanged in substance.

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

That fourth row matters: per the comment at
[timeline.html:65142-65146](../../timeline.html#L65142-L65146), the local-dev `PUT` handler runs
deduplication and detection *synchronously before responding*, so byte progress reaching 100% does
not mean the wait is over. A bar sitting full while the server works looks frozen, so the label
changes to say what's happening.

Mechanism:
- **Upload progress needs `XMLHttpRequest`**, not `fetch` — `fetch` has no upload-progress facility.
  Only the `PUT` at [timeline.html:65139](../../timeline.html#L65139) changes; every other call
  stays on `fetch`. A built-in browser API beats adding an upload library under this repo's
  reuse-order rule.
- **Download progress** uses `fetch` + `response.body.getReader()` against `Content-Length`; with no
  such header, show transferred bytes rather than a fabricated percentage.
- **ETA** from a rolling ~3-second window of progress events, not a whole-transfer average;
  suppressed until at least two samples exist; rendered as `about 20 seconds left`, never `18.4s`.
- **Markup reuses the page's existing progress idiom** — `.progress-track` / `.progress-fill` /
  `.progress-label` at [timeline.html:534-553](../../timeline.html#L534-L553), already used by the
  AI-classify bar ([:849-852](../../timeline.html#L849-L852)) and analytics
  ([:66451-66452](../../timeline.html#L66451-L66452)). No new CSS; `.is-error` already exists for
  the failure state.

Phase 1 also makes this phase faster in practice: the discarded per-message detection pass is gone,
so the client-side parse step shrinks.

---

## Phase 3: Back means "back one step", and leaving the page stops being expensive

### Hash routing

Nothing in the page touches the History API today — no `pushState`, `hashchange`, or `popstate`
anywhere — so tab changes, conversation selection
([timeline.html:65906](../../timeline.html#L65906)), and analytics selection are invisible to the
browser, and Back exits the page entirely.

Write that state to `location.hash` (`#calendar`, `#conversations/42`, `#analytics/friction`) and
restore from it on `hashchange`. This gives real Back/Forward plus bookmarkable deep links.

**Hash, not `pushState`, specifically because the page is still opened as a `file://` URL** — the
e2e suite does exactly that at [e2e/upload-flow.spec.js:13](../../e2e/upload-flow.spec.js#L13) — and
`pushState` is restricted for file URLs in some browsers, while hash works everywhere.

Scope: the three navigation axes above. Review-table pagination and search filters stay out of the
hash for now (they'd churn history on every keystroke); revisit if you want them.

### Restore on load, using the affordance that already exists

Even with history wired up, a refresh or a Back past the first entry lands on the file picker with
everything gone. The backend still holds the processed export behind `GET /export`, so:

- On startup, if a dev login name is remembered in `localStorage`, call `GET /export` and render
  directly — no file picking, no re-upload.
- If that returns nothing, or the server was restarted (its storage is in-memory), fall back to the
  load screen exactly as today.
- **The existing "Load a different file…" button** ([timeline.html:797](../../timeline.html#L797),
  handler at [:65237-65243](../../timeline.html#L65237)) is the affordance for getting back to the
  picker — it already does precisely that. It needs rewiring only insofar as it must also clear the
  remembered session so the next startup doesn't silently restore the old export again.
- The dev login name must be persisted to `localStorage` on successful login for any of this to
  work; it currently isn't.

---

## Phase 4: dev-server launcher and VS Code task

Carried forward from the superseded launcher plan, including your correction that unconditional
restarts are wrong.

`scripts/dev-up.sh {backend|static}`. The two roles need different logic, because only one has
anything that can go stale:

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
   - **Listening but hash differs, or it's something else squatting** → print the occupant's PID and
     full command line, `SIGTERM`, wait 3s, `SIGKILL` if needed, then start fresh.

Why this matters more than saved seconds: `timeline-api` holds every upload and confirmed flag **in
memory only**, so an unnecessary restart silently deletes whatever you were looking at.

**`static`** — `python3 -m http.server` re-reads `timeline.html` from disk on every request, so a
running instance can never serve stale content. Liveness-only: if it's up and answering
`curl -sf .../timeline.html`, leave it; otherwise (nothing there, or a wrong process) clear and
start.

`scripts/dev-down.sh {backend|static|all}` — unconditional clearing, for manual use and for the
test's teardown.

**VS Code tasks** (`.vscode/tasks.json`): one leaf task per role (`isBackground: true`, problem
matchers keyed to `listening on http://` and `Serving HTTP on`), plus a compound
`"Dev: Start local environment"` with `dependsOrder: parallel`, marked as the default build task so
"Run Build Task" triggers it — no extension needed, and it works in VS Code Web.
`"presentation": {"reveal": "silent"}` keeps a no-op "already up to date" run from stealing focus.

Note what a task pane means under this design: a live pane is *sufficient* evidence the server is
running but no longer *necessary*, since most reruns find it current and exit without attaching.
Liveness truth stays with `lsof`.

**Testing** (`scripts/test-dev-up.sh`, run manually — it manages real ports and processes):
1. *Stale occupant*: park a throwaway `python3 -m http.server 3000`, run the script, assert the real
   `timeline-api` replaced it (one process on the port, right command line).
2. *Already current*: note the PID, rerun with no source change, assert the **PID is unchanged** and
   that **no `kill` was invoked at all** (run under a stubbed `kill` that records calls). This is the
   test that catches the data-loss bug your correction identified.
3. *Genuinely changed*: touch a file under `backend/timeline-core/src/`, rerun, assert the PID
   changes and the new process answers `/conversations` — proves restarts aren't permanently
   disabled.
4. Same for the static server, minus case 3 (staleness doesn't apply).
5. Teardown via `dev-down.sh all` in a trap.

---

## Sequencing

The order above follows your instruction that the page gets stripped first. One suggestion worth
considering: **Phase 4 is independent of the other three and worth pulling to the front**, because
Phases 1-3 involve restarting the backend repeatedly while testing, and Phase 4 is precisely what
makes those restarts cheap and non-destructive. Your call — nothing breaks either way.

## Self-critique log

### C1 [OPEN]: the client-side parse phase still can't show real progress
*(carried forward from the progress plan's C1)* `parseUploadedConversations` is synchronous
main-thread work over the whole export, so even an indeterminate label can't animate during it. Real
progress needs chunked parsing or a Web Worker — both materially bigger than this plan.
**Mitigation in plan:** the phase gets an honest label rather than a fake bar, and Phase 1 removes
the largest avoidable part of its cost outright.
**Open:** revisit if the parse is still a visible freeze after Phase 1 lands. That measurement is the
trigger and can't be taken until then.

### C2 [RESOLVED]: `fetch` can't report upload progress
*(progress plan C2)* **Resolution:** the single `PUT` carrying the file body moves to
`XMLHttpRequest` for `upload.onprogress`; everything else stays on `fetch` — see
[§Phase 2](#phase-2-real-progress-for-the-upload-honest-labels-elsewhere).

### C3 [RESOLVED]: a naive ETA is worse than none
*(progress plan C3)* **Resolution:** rolling ~3-second window, suppressed until two samples exist,
rendered in rounded hedged language — see [§Phase 2](#phase-2-real-progress-for-the-upload-honest-labels-elsewhere).

### C4 [RESOLVED]: deferring the drift check was the wrong answer
*(supersedes the progress plan's C5, which argued for keeping the check as a JavaScript-vs-Rust
consistency signal during the migration)* Original concern: making the check lazy must not amount to
quietly deleting a useful signal. **That reasoning doesn't survive Phase 1.** Keeping a second
detection implementation in the browser purely so it can disagree with the real one inverts the point
of the migration — the backend is meant to be the only implementation. What's genuinely lost: the
live divergence signal on real user data. What covers it instead: `timeline-core`'s own tests, and
the e2e suite asserting that backend-produced flags render.
**Resolution:** the drift modal, `showDriftModal`, and `driftCount` are deleted in
[§Phase 1](#what-gets-deleted), not deferred.

### C5 [RESOLVED]: unconditionally killing the running backend is destructive
*(launcher plan C6, raised by you)* `timeline-api` keeps all state in memory, so "always kill, then
start fresh" discarded the user's session on every rerun, including reruns that only meant to check
whether anything needed restarting. **Resolution:** build-hash comparison per role, with the static
server exempt from staleness entirely — see [§Phase 4](#phase-4-dev-server-launcher-and-vs-code-task);
the skip-and-restart branches are both explicit test cases.

### C6 [RESOLVED]: killing "whatever is on the port" could hit an unrelated process
*(launcher plan C2)* **Resolution:** scoped to two project-specific ports, never a system-wide scan,
and the occupant's PID and full command line are printed before any signal — and after C5, a signal
is only sent when the occupant is actually stale or actually wrong.

### C7 [RESOLVED]: `pushState` would break the `file://` use case
Original concern: standard SPA routing reaches for `pushState`, but this page is still loaded as a
`file://` URL by the e2e suite, where `pushState` is restricted in some browsers — history routing
built that way would work in manual testing and fail in the test that's supposed to protect it.
**Resolution:** `location.hash`, which works identically under `file://` and `http://` — see
[§Phase 3](#hash-routing).

### C8 [OPEN]: restore-on-startup changes what the page does when you open it
Phase 3's restore path means the page no longer always opens on the load screen. If the remembered
session is stale or unwanted, a user could reasonably be confused about why old data appeared.
**Mitigation in plan:** the existing "Load a different file…" button is the escape hatch and already
does the right thing, needing only to also clear the remembered session.
**Open:** whether restore should be silent or announced (e.g. a dismissible "restored your last
export" line). Trigger: your preference — this is a judgment call about your own workflow, and it's
cheap to change either way.

### C9 [OPEN]: Phase 1 is a large deletion with the e2e suite as its only automated net
Cutting 85% of a file in one pass is exactly where something quietly breaks. The e2e suite covers the
upload → render → flag → reload path, but not the calendar, the analytics views, or the review
table's filters.
**Mitigation in plan:** the deletion is confined to detection and its lexicons, and four separate
facts were verified in source establishing that nothing else calls them.
**Open:** whether to add e2e coverage for the calendar and analytics views *before* Phase 1 rather
than after. Recommended, and it's the safer order, but it's additional work you haven't asked for —
your call. Trigger: decide before Phase 1 starts.

## Open questions for review

1. **The e2e drift-modal block** ([e2e/upload-flow.spec.js:63-66](../../e2e/upload-flow.spec.js#L63-L66))
   must be deleted along with the modal. That's a committed test, so it needs your explicit
   approval — separate from approving this plan.
2. **The "Automatically detect flags" load-screen checkbox** — recommend deleting it, since the
   backend always detects and the existing "Show automatic tags" global toggle already covers the
   display side. Confirm, or say if you'd rather keep it doing something.
3. **C9's ordering question**: add calendar/analytics e2e coverage before the big deletion, or accept
   the current net?
4. **Phase 4 first?** See [§Sequencing](#sequencing).
5. **`scripts/` + bash** as the home for Phase 4, versus a `justfile`/`Makefile`. Bash chosen only
   because the README already documents raw shell commands.
