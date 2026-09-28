# Upload progress bar, and making the detection-drift check pay-on-demand

Two separate complaints from the same load flow, both about cost the user can see but can't
control:

1. Choosing a file shows `Uploading your conversation export…`
   ([timeline.html:65130](../../timeline.html#L65130)) with no percentage, no bar, and no estimate —
   for a real export (this project's own was 64.7MB) that's a long, silent wait, worse over a
   proxied/remote connection like the Tailscale-forwarded setup.
2. Once loading finishes, a modal announces exactly how many messages *would* be flagged
   differently — which gives away that the alternative flags were already computed for every
   message, before the user expressed any interest in them. The user only wants that cost paid if
   they click "Use updated detection."

## Part 1: real progress for the upload, honest indeterminacy everywhere else

### What each phase can actually report

`handleLoadClick` ([timeline.html:65115-65204](../../timeline.html#L65115-L65204)) runs six phases.
They differ in whether progress is *observable at all*, and the design says so rather than
inventing numbers:

| Phase | Observable? | Shown |
|---|---|---|
| `_dev/login` | No — single fast request | Label only, no bar |
| `POST /uploads` | No — single fast request returning a URL | Label only, no bar |
| `PUT {upload_url}` (the bytes) | **Yes** — real byte counts | **Real bar + % + ETA** |
| Server-side processing tail of that same PUT | No — request body fully sent, response not yet back | Bar pinned at 100%, label switches to `Finishing up on the server…` |
| `GET /export` | No — fast, returns a URL | Label only |
| `GET {export_url}` (download) | **Yes, if `Content-Length` is set** | Real bar + %, else running byte count |
| `parseUploadedConversations` (client CPU) | Not without restructuring | Label only — see C1 |

The "server-side processing tail" row matters and isn't a detail: per the existing comment at
[timeline.html:65142-65146](../../timeline.html#L65142-L65146), the local-dev `PUT` handler runs
deduplication and flag detection *synchronously before responding*. So the upload request's byte
progress hitting 100% does **not** mean the wait is over — there's a further, unobservable stretch
where the server is working. Leaving a bar sitting at 100% during that would look frozen, so the
label changes to say what's actually happening.

### Mechanism

- **Upload progress needs `XMLHttpRequest`, not `fetch`.** `fetch()` has no upload-progress
  facility (its `ReadableStream` request bodies are not portably supported, and there's no
  equivalent of `xhr.upload.onprogress`). `XMLHttpRequest` is a built-in browser API, so per this
  repo's reuse-order rule this beats adding any upload library. Only the `PUT {upload_url}` call at
  [timeline.html:65139](../../timeline.html#L65139) changes; every other `fetch` in the flow stays
  as-is.
- **Download progress uses `fetch` + `response.body.getReader()`**, summing chunk lengths against
  the `Content-Length` header. When that header is absent, show transferred bytes (`Downloaded
  12.4 MB…`) instead of a percentage — never a fabricated one.
- **ETA** = remaining bytes ÷ a transfer rate measured over a **rolling window of the last ~3
  seconds** of progress events, not a whole-transfer average (which lags badly after a slow start
  and then reads as stubbornly wrong). Displayed rounded and hedged — `about 20 seconds left`,
  `about a minute left` — never `18.4s`, which claims precision the estimate doesn't have.
  Suppressed entirely until at least two samples and ~1s have elapsed, since a rate computed from
  one event is meaningless.

### Markup and styling: reuse, don't invent

The page already has a progress-bar idiom used twice — AI classification
([timeline.html:849-852](../../timeline.html#L849-L852)) and analytics
([timeline.html:66451-66452](../../timeline.html#L66451-L66452)) — backed by `.progress-track` /
`.progress-fill` / `.progress-label` CSS at
[timeline.html:534-553](../../timeline.html#L534-L553), including an `.is-error` state. The new
load-screen bar reuses exactly those classes, adding no CSS:

```html
<p class="load-status" id="loadStatus"></p>
<div id="loadProgress" style="display:none;">
  <div class="progress-track"><div class="progress-fill" id="loadProgressFill"></div></div>
  <div class="progress-label" id="loadProgressLabel"></div>
</div>
```

`setLoadStatus` keeps its current job (phase text + error colouring). Three small helpers manage the
bar: show-indeterminate (hide the track, label only), show-determinate (percent + ETA), and hide.
On failure the existing `.is-error` class turns the bar red rather than leaving it mid-fill.

## Part 2: the drift check becomes pay-on-demand

### What it costs today, and why it's paid twice

In `parseUploadedConversations` ([timeline.html:65001-65017](../../timeline.html#L65001-L65017)),
when a message has stored automatic flags and the user did *not* tick "refresh detection," the code
still runs `findEmphasisCapsWords`, `detectCritical`, and `detectAngry` on that message — three
heuristic passes including sentiment scoring — purely to compare against the stored values and
increment `driftCount`. The freshly computed values are then **thrown away**; only the count
survives, to populate the modal's text at
[timeline.html:65210-65211](../../timeline.html#L65210-L65211).

Then, if the user *does* click "Use updated detection," the handler at
[timeline.html:65219-65233](../../timeline.html#L65219-L65233) calls
`parseUploadedConversations(rawText, runAutoDetect, true)` — re-running `JSON.parse` over the entire
raw export *and* re-running all three heuristics over every message a second time. So the current
code pays for detection twice on the path where the user says yes, and once-and-discarded on every
path where they say no or are never asked.

Worth noting what this check actually compares, since it isn't self-evident: the stored automatic
flags in a freshly-uploaded file come from the **Rust backend's** detection pass during
`process_upload`, while the fresh values come from this page's **JavaScript** implementations. The
drift modal is, in practice, a live consistency check between the two implementations during the
migration — as the e2e test's own comments about the AFINN→VADER threshold difference describe
([e2e/upload-flow.spec.js:50-62](../../e2e/upload-flow.spec.js#L50-L62)). That makes it worth
*keeping* as a capability, which is why this plan defers it rather than deleting it.

### New flow

1. **Loading computes no heuristics for comparison.** `parseUploadedConversations` drops the
   drift-counting branch entirely and instead tracks one cheap boolean while it's already walking
   the messages: did any message arrive with `_claude_timeline_auto.source === 'heuristic'`? That's
   a field read, not a computation.
2. **If so, show a quiet banner, not a modal** — once the main view renders, near the top:
   *"Some automatic tags in this file were saved by an earlier detection pass. [Check whether
   current detection would flag anything differently]"*. Non-blocking, dismissible, costs nothing
   to display.
3. **The check runs only on that click.** A new `computeDetectionDrift()` walks the already-parsed
   `HUMAN_MESSAGES` (no `JSON.parse`, no re-download — the text and stored flags are already in
   memory), runs the three heuristics *once* over the messages whose `auto_source === 'heuristic'`,
   and returns both the count of differences and a `Map` of message id → freshly computed flags.
4. **Zero differences** → an inline note ("Current detection agrees with everything saved here"),
   no modal, banner dismissed.
5. **Some differences** → the existing `driftModal`
   ([timeline.html:901-909](../../timeline.html#L901-L909)) opens with the real count and its
   existing two buttons. "Keep saved tags" discards the `Map`. "Use updated detection" applies the
   `Map`'s already-computed values straight onto `HUMAN_MESSAGES` and re-renders — **no third
   heuristic pass, no re-parse**, fixing the double-computation described above.

Net effect: a user who never clicks pays nothing; a user who clicks pays exactly once.

The existing "Refresh automatic tags saved in this file" checkbox on the load screen
([timeline.html:768-780](../../timeline.html#L768-L780)) is unaffected — ticking it before loading
still means "just use fresh detection, don't ask me," which is an explicit up-front opt-in and so
already consistent with the pay-on-demand principle.

## Self-critique log

### C1 [OPEN]: the client-side parse phase still can't show real progress
`parseUploadedConversations` is synchronous main-thread work over the whole export — with a large
file it blocks the UI, so even the indeterminate label can't animate during it. Showing true
progress would mean either chunking the loop with `requestAnimationFrame`/`setTimeout` yields or
moving it into a Web Worker; both are materially bigger changes than this plan, and the Worker
route would need the heuristic functions factored out of the page's single global scope.
**Mitigation in plan:** the phase gets an honest label rather than a fake bar, and Part 2 removes
the biggest avoidable chunk of its cost (the discarded heuristic pass) outright, which should
shorten it noticeably on files with stored automatic flags.
**Open:** revisit if the parse phase is still a visibly long freeze after Part 2 lands — that
measurement is the trigger, and it can't be taken until Part 2 is running.

### C2 [RESOLVED]: `fetch` can't report upload progress
Original concern: the natural move is to keep `fetch` everywhere for consistency, but it has no
upload-progress callback, so a progress bar built on it would have nothing real to display.
**Resolution:** the single `PUT` that carries the file body switches to `XMLHttpRequest` for its
`upload.onprogress` events; everything else stays on `fetch` — see
[§Mechanism](#mechanism). A built-in browser API is preferred to any third-party uploader under
this repo's reuse-order rule.

### C3 [RESOLVED]: a naive ETA is worse than none
Original concern: a lifetime-average rate produces an estimate that stays visibly wrong for a long
time after any speed change, and rendering it to sub-second precision implies confidence the number
doesn't have.
**Resolution:** rolling ~3-second window, suppressed until there are at least two samples, rendered
in rounded hedged language — see [§Mechanism](#mechanism).

### C4 [RESOLVED]: a bar that sits at 100% while the server works looks frozen
Original concern: because the local-dev `PUT` handler processes the upload before responding, byte
progress reaching 100% leaves a silent gap with the bar full and nothing happening.
**Resolution:** at 100% the label switches to `Finishing up on the server…` and the bar goes
indeterminate rather than pretending completion — see the phase table in
[§What each phase can actually report](#what-each-phase-can-actually-report).

### C5 [RESOLVED]: deferring the check shouldn't mean losing it
Original concern: the drift check is doing real work during the migration — it's the only live
signal that the page's JavaScript detection and the Rust backend's detection disagree on actual
user data. Making it lazy must not amount to quietly deleting it.
**Resolution:** kept in full, same modal and same two choices, moved behind an explicit banner
click; the rationale is recorded in [§What it costs today, and why it's paid
twice](#what-it-costs-today-and-why-its-paid-twice) so the next reader doesn't mistake it for dead
UI and remove it.

### C6 [RESOLVED]: "Use updated detection" recomputed everything from scratch
Original concern: the existing handler re-parses the entire raw JSON and re-runs every heuristic,
duplicating work the drift check just did — a pre-existing inefficiency in the exact code this plan
rewrites.
**Resolution:** `computeDetectionDrift()` returns the computed values alongside the count, and the
"Use updated detection" path applies them directly — see [§New flow](#new-flow), step 5. Adjacent
to the requested change and in the same function, so folded in rather than left behind.

## Open questions for review

- **Banner wording and placement.** Proposed: a single line just under the page header, above the
  tab bar, so it's visible from any tab. The alternative is putting it inside the "Review & flags"
  tab only, where flags actually live — quieter, but easy to never notice.
- **Should a zero-difference result be remembered?** If the check finds nothing, re-showing the
  banner after every subsequent action would be noise; proposed behavior is to replace the banner
  with the inline "agrees with everything saved" note for the rest of that session.
- **Is Part 1's scope right?** It gives real progress for the two phases that genuinely have
  measurable byte counts and honest labels elsewhere. If what you want is a single bar that sweeps
  0→100% across the *whole* load (including the parse phase), say so — that's the C1 work, and it's
  a bigger change involving either chunked parsing or a Web Worker.
