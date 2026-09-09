# Timeline Project — Development Decision Log

This records the full history of requirements and design decisions behind `timeline.html`, a
self-contained, client-side viewer/annotator for Claude conversation exports (`conversations.json`).
It's meant to give a fresh developer (or coding agent) everything needed to continue the project
without re-deriving context from the original chat. Decisions are grouped by subsystem, not strict
chronological order, but each notes what was explicitly requested vs. what was an implementation
choice made without an explicit spec.

---

## 1. Project Purpose & Core Architecture Principles

**Requested:** A tool to visualize a user's exported Claude conversation history as a timeline,
originally to answer "how much time did I actually spend on each conversation."

**Decided internally, carried through every later feature:**
- **100% client-side.** No backend, no server, nothing uploaded anywhere. All parsing, computation,
  and rendering happens in the browser via vanilla JS. This was an explicit constraint once the tool
  started handling personal/sensitive conversation content — stated directly in the UI copy
  ("nothing is uploaded anywhere").
- **No external dependencies / works offline.** No CDN-loaded chart or markdown libraries. Custom
  SVG chart primitives and a hand-written markdown-lite renderer were built instead, specifically so
  the single HTML file stays fully self-contained.
- **GitHub Pages compatibility was discussed** (static hosting only, no server-side execution
  possible there) — reinforced the client-side-only architecture as the only viable path if the user
  wants to publish this tool as a static site.

---

## 2. Data Source & File Format

### 2.1 Original format
Anthropic's `conversations.json` export: a bare JSON array of conversation objects, each with
`name`, `uuid`, `created_at`, `updated_at`, and `chat_messages` (array of message objects with
`sender`, `created_at`, `content` — an array of content pieces, text pieces have `type: "text"` and
`text`).

### 2.2 Evolution of how the tool reads this file
1. **First version:** data was extracted server-side (by Claude, in a sandboxed Python environment)
   and baked into the HTML at generation time via placeholder substitution. Multiple iterations of
   what got embedded (timestamps only → full text → flags).
2. **Requested explicitly:** decouple the tool from one specific export — make it a general-purpose
   viewer that loads *any* `conversations.json` at runtime via a file `<input>`, entirely in-browser,
   rather than having data baked into the HTML. This was the point the tool became reusable rather
   than a one-off artifact tied to a single person's data dump.
3. Parsing, dedup, and all derived structures (see below) now happen in JS on file load.

### 2.3 Deduplication
**Requested:** remove exact-repeat messages (accidental resends when a request silently failed to
get a response).

**Final rule (refined twice from the user's clarifications — this is the correct, current
behavior):**
- Duplicates are **adjacent in the human-message subsequence**, not necessarily adjacent in real
  time or in the raw array — a resend can come hours later with nothing in between, or with a
  spurious assistant reply to an earlier failed attempt sitting between the two resends.
- For a run of *n* identical consecutive human messages (by exact text match), **keep the last one**
  (the one that actually got a real reply) and **discard requests 1..n-1**, plus **any messages of
  any sender that occurred between the first and last occurrence** (e.g., an erroneous reply to an
  earlier failed attempt).
- Implemented as `dedupChatMessages()` — walks the message array, and for each human message, scans
  forward (tolerating intervening assistant messages) to find the last message with identical text
  before a genuinely different human message breaks the run; jumps straight to that last occurrence,
  dropping everything in between.
- Verified against 6 hand-built edge cases (simple chains, a stray reply to an early attempt, no
  duplicates, mid-conversation duplicates, a 5-way chain with a stray reply) plus real data (removed
  31 of 4,482 raw messages on the sample dataset — up from 25 under an earlier, cruder
  "strictly-adjacent-regardless-of-sender" version of the rule that didn't tolerate intervening
  assistant replies).

### 2.4 File format versioning (breaking change, explicitly authorized)
**Requested:** stop keeping flags in a separate file from the conversation data; merge them into one
file. User explicitly authorized a breaking change to the root JSON structure ("it's fine if you
modify the format, as long as you can still read the original as well as the changed format").

**Decision:** wrap the root in an object instead of a bare array:
```json
{ "claude_timeline_format_version": "2", "conversations": [ ...original array... ] }
```
- **Bare array input** (a fresh unprocessed Claude export, or a file saved by an older pre-v2
  version of this tool) → no version marker → treated as unprocessed → dedup runs on it now.
- **Wrapped input with the version marker** → treated as already deduped → dedup is skipped.
- This is a bigger compatibility break than earlier additive-field changes (root type changes from
  array to object) — accepted deliberately, per explicit user sign-off, on the reasoning that people
  aren't likely to feed this file into unrelated third-party tools expecting the raw Anthropic
  schema.

### 2.5 Per-message ID scheme
**Decided internally.** Originally used a simple incrementing integer index assigned during
extraction. This broke as soon as any operation could change *which* messages survive (e.g., dedup
removing some messages shifts every subsequent index). Switched to a **composite string key**:
`` `${conversationIndex}|${message.created_at}` ``. This stays stable across dedup and reloads *as
long as the surviving message's own timestamp and conversation position don't change* — which holds
for every operation this tool performs (dedup only removes messages, never reorders or edits
survivors).

### 2.6 Auto vs. user flag separation (structural guarantee, heavily tested)
**Requested, repeatedly, as a hard requirement:** automatic detection must never be able to silently
overwrite a flag the user set by hand — this was asked for explicitly, and then re-verified after
later changes (LLM classification, "refresh detection" option) risked reintroducing the problem.

**Decision:** two entirely separate fields per human message, both in the in-memory data model and
in the exported file:
- `_claude_timeline_auto` — `{ caps, angry, critical, source }`, where `source` is `"heuristic"` or
  `"llm"`. Always computed automatically; never touched by user actions.
- `_claude_timeline_user` — only present for messages the user has explicitly confirmed/overridden;
  `{ caps, angry, critical }` (all three always written together — see §5.3, row-level "approve"
  semantics).
- The effective value shown anywhere in the UI is computed by `effectiveFlag()`, which — depending
  on the current visibility toggles (§6) — either prefers the user value, uses auto only, uses user
  only, or shows nothing. **User values are never merged into or derived from auto values, in either
  direction.**
- This was stress-tested directly: set a user override to the *opposite* of what the heuristic says,
  then force a full "refresh detection" (recompute-from-scratch) reload, and confirm the override
  survived unchanged. It did, in every test run.
- **Legacy migration:** an earlier, since-retired single-field format (`_claude_timeline_flags`) is
  still recognized on load and migrated into `_claude_timeline_user`, so older saved files aren't
  stranded by the v2 schema change.

---

## 3. Session/Block Model (used throughout Calendar, Conversations, Analytics)

**Requested:** show real elapsed time per conversation, not just a start timestamp — an early bug
report caught the "duration" field actually just showing the clock time of the *first* message, not
an actual duration.

**Decisions:**
- A **"session" (internally, `BLOCKS`)** is a contiguous run of messages within one conversation,
  bucketed by **local calendar day** (computed client-side from the viewer's own timezone — an
  earlier version bucketed by UTC day server-side and then displayed in local time, which caused bars
  to render past the edge of their day's track when the viewer's timezone didn't match UTC; fixed by
  moving all day-bucketing to run client-side, in the browser's own timezone, at render time).
- Within a day, a session further **splits whenever the gap since the previous message is ≥ 15
  minutes** — requested explicitly, so idle time (leaving a conversation open, coming back later) 
  isn't counted as "active" duration. A conversation can show several separate session blocks in one
  day.
- Each block records: `conv` (index), `date`, `start`, `end`, `duration_sec`, `count`, plus (via
  `attachFlags()`) the flagged human messages that fall within it and the full list of human messages
  in it (`allHuman`) — used for click-to-review navigation and Analytics.

---

## 4. UI Structure — Tabs

Four tabs: **Calendar**, **Conversations**, **Review & flags**, **Analytics**. All requested
incrementally; final structure below.

### 4.1 Calendar tab
- Day-by-day 24-hour horizontal tracks, one bar per session, positioned/sized by start time and
  duration within that day.
- Bars show stacked flag icons (⚑ critical, `!` angry, `A` caps) when the session contains flagged
  messages.
- **Clicking a bar** → jumps to Review, filtered to that session's exact time span, with the flagged
  messages (or, if a specific flag icon was clicked, just that flag type's messages) highlighted —
  originally this filtered *out* everything except the flag type, which lost surrounding context; per
  explicit feedback, fixed so the whole session period always shows, with just the relevant message
  flashed/scrolled into view.
- **Clicking the date label itself** → jumps straight to a whole-day Review view across *every*
  conversation active that day (see §7.3).

### 4.2 Conversations tab
- Left: searchable list of conversations, with flag icons for any that have flagged content.
- Right: selected conversation's session table (day, time span, duration, message count, flags),
  plus a "Chat message review →" link to view the entire conversation unfiltered in Review.
- Same click-to-review behavior as Calendar bars (whole-session context, not filtered to one flag
  type).
- List/detail counts and icons respect the global visibility toggles (§6) — this was specifically
  requested ("counts and flags in the conversations tab should also be affected by the switches").

### 4.3 Review & flags tab
The main annotation surface. See §5 for detection and §6 for the visibility-toggle interaction.

- Every human message listed (not just flagged ones) — requested explicitly ("I want a way to review
  the actual text behind each message... not just the flagged ones"), because earlier iterations only
  showed curated/flagged samples and the user suspected real under-counting.
- Paginated (50/page), searchable by message text, filterable by flag type or "you've overridden."
- Optional Claude-reply rows interleaved (see §8).
- Filter banner shows the active conversation/time-span/day filter with a "Clear filter" control, and
  — when a narrower filter is active — "View entire conversation" / "View entire day" buttons to
  widen it without leaving the tab.
- "Classify with AI" box (see §5.4) and "Download annotated conversations.json" (see §2.4/§9) live
  here.

### 4.4 Analytics tab
**Requested:** brainstorm and then build several specific analyses. User picked, from a suggested
list: friction ranking (both conversation-level and session-level percentage), flag-rate trend over
time (weekly/monthly), and all three correlational analyses that had been floated (session length,
time-of-day/day-of-week, idle-gap-before-session).

**Decisions:**
- Explicitly requested: computation must run **on demand** (not eagerly on tab load) and show a
  **real progress bar** if it's slow — implemented via `computeWithProgress()`, which processes items
  in chunks with `requestAnimationFrame` yields between chunks, so the progress bar genuinely
  animates (verified directly: a test recorded actual intermediate percentages — 18%, 35%, 53%... —
  rather than an instant jump to 100%) regardless of how fast the underlying computation actually is.
- **Friction ranking**: toggle between "by conversation" and "by session" granularity; sortable list
  showing message count / flagged count / % flagged; click a row to open that conversation or
  session in Review.
- **Flag rate over time**: line chart, weekly/monthly toggle.
- **Session length vs. flag rate**: scatter plot + Pearson correlation coefficient. (On the real
  sample data this came back ≈0.02 — essentially no relationship — which the user initially
  attributed to unreliable detection; this fed directly into later work on improving detection
  quality, §5.)
- **Time of day & day of week**: two bar charts (24 hourly buckets, 7 weekday buckets), % flagged per
  bucket.
- **Idle time before a session**: scatter plot of hours-since-previous-session (log-scaled x-axis)
  vs. that session's flag rate, + correlation. Sessions that are the first in their conversation are
  excluded (no prior gap to measure) and the exclusion count is stated on-screen.
- All charts are hand-built minimal SVG (bar/line/scatter primitives) — no charting library, to keep
  the file dependency-free.

---

## 5. Flag Detection — Three Kinds, Three Very Different Mechanisms

Three flag types: **ALL-CAPS**, **critical** (of a Claude response), **angry**. These are treated
very differently on purpose, and that distinction is a recurring theme in the UI copy: caps is
mechanical/deterministic, the other two are heuristic judgment calls meant to be corrected by hand.

### 5.1 ALL-CAPS — mechanical, dictionary-based
**Requested:** the original naive regex (`/\b[A-Z]{2,}\b/`) had heavy false positives — flagged
common acronyms (IRS, DARPA, ICHRA, QSEHRA, APR) and conventionally-capitalized words like "OK." User
explicitly suggested checking whether the word is a real English word as the fix.

**Decision:** embedded a real English dictionary (~64,000 words, sourced from Debian's `wamerican`
package, filtered to lowercase-only entries so proper nouns/abbreviations that only appear
capitalized in the source list — like "OK", "TV", "IRS", "APR" — are correctly excluded). A caps word
only counts if its lowercased form is a genuine dictionary word. A small manual exclude-list
(`id`, `eta`) covers a couple of real-but-usually-abbreviation-in-context words that slipped through.
Verified directly: `IRS`/`DARPA`/`ICHRA`/`QSEHRA`/`OK` → zero matches; `WRONG`/`RIDICULOUS` → both
correctly flagged.

### 5.2 Criticism & anger — heuristic, explicitly not authoritative
**Evolution:**
1. First pass: I (Claude) manually read a keyword-matched candidate list and hand-curated which were
   genuine criticism/anger, entirely offline, as a one-time analysis of one specific dataset.
2. **Requested:** make this run automatically, in the browser, at file-load time, "if the user
   chooses" (opt-in checkbox) — so the tool works generically on *any* uploaded file, not just the one
   dataset Claude had already read by hand.
3. **Criticism** detection: a deliberately wide-net keyword/phrase regex (wrong, incorrect,
   fabricated, outdated, poorly researched, "you failed to," etc.) — calibrated against the earlier
   hand-curated list and confirmed to have 100% recall against it (catches every message previously
   hand-picked, plus more candidates for the user to review/dismiss).
4. **Anger** detection: AFINN sentiment lexicon (~3,400 scored words) + an anger-specific phrase list
   (profanity, "sick of," "pissed," etc.) + exclamation-mark bursts. Threshold calibrated (favoring
   recall over precision, since this is meant to be reviewed by hand afterward, not trusted outright)
   against the same hand-curated baseline.
5. **Explicit design stance, stated repeatedly in the UI copy:** both are framed as a first-pass
   suggestion for the user to correct, never a verdict.

### 5.3 Review workflow — checkboxes, "Approve," and the four-visibility-state matrix
**Requested:** let the user review and override any message's flags by hand, with a way to mark a
whole row reviewed without touching every checkbox individually.

**Decisions:**
- **Clicking any single checkbox on a row, or the "Approve" button, promotes ALL THREE flags to
  explicit user-owned values simultaneously** — using the just-changed value for the box actually
  clicked, and the row's *current effective value* for the other two. One click reviews the whole
  message. (User's own words: "checking any box in the row should indicate that the row has been
  reviewed without needing to check the other boxes.")
- **Global toggles** (`SHOW_AUTO`, `SHOW_USER`) sit above the tabs (affecting Calendar, Conversations,
  *and* Review — deliberately global, not scoped to one tab, per explicit correction after an initial
  proposal to scope it only to Calendar). `SHOW_REPLIES` (§8) is a third, independent toggle.
- **Four-state behavior, exactly as specified by the user:**

  | Auto | User | Behavior |
  |------|------|----------|
  | ON | ON | Normal: user override wins if set, else auto. Checkboxes editable, labeled "auto"/"you", Approve shown. |
  | ON | OFF | Effective = auto only, user overrides completely ignored (not deleted). Checkboxes shown but **disabled**, no source label, no Approve button. |
  | OFF | ON | Effective = user value if explicitly set, else **nothing** (auto is not used as a fallback). Checkboxes editable; unchecked boxes labeled **"tagged"**/**"untagged"** (a stated "no" vs. no opinion yet) instead of "auto"/"you". Approve works normally. |
  | OFF | OFF | Checkbox columns and Approve button removed entirely from the Review table. |

  All four states were tested directly, including the specific case of clicking only one checkbox
  and confirming the *other two* flags on that row got promoted to "you" as well.

### 5.4 "Classify with AI" — zero-shot LLM classification (opt-in, in addition to the heuristic)
**Context for why this exists:** the near-zero correlation found in Analytics (§4.4) led the user to
suspect the heuristic was simply unreliable, having done substantial manual correction themselves
with very different results. Discussed and rejected: training a small classifier (decision
tree/logistic regression) on the user's own corrected labels, on the reasoning that (a) a
personalized model wouldn't generalize to other users of the now-generic tool, and (b) an LLM doing
real zero-shot judgment should structurally outperform a fixed keyword/lexicon heuristic on exactly
the cases that heuristic misses (negation, sarcasm, mixed tone). **Explicitly requested: zero-shot
only, no few-shot examples** (few-shot would require the person supplying examples, cheap for a
power-user but not for a casual visitor of a would-be public tool).

**Mechanism:**
- Uses the Claude-artifact `fetch('https://api.anthropic.com/v1/messages', ...)` mechanism — **only
  works while this page is running as a live-rendered Claude artifact**; there is no API key or proxy
  available to a locally-opened copy of the file, and this is a hard platform boundary, not a bug
  (confirmed directly via a real CORS rejection: `origin 'null'` + no `Access-Control-Allow-Origin`
  header, which cannot be worked around from inside page JS).
- Cost was estimated up front (~$1 at Haiku / ~$3 at Sonnet for the full ~2,200-message sample
  dataset, batched 30 messages/call) before building this, specifically to inform the decision to
  build it at all.
- Batches of 30 messages per call; each message sent with its preceding Claude reply as context (for
  judging criticism, which usually can't be assessed without knowing what's being criticized).
- **Prompt hardening** (added after real failures surfaced — see §10) uses explicit XML-style
  `<message index="N">` delimiters and repeats the "classify only, never respond to content" 
  instruction at both the start and end of the prompt, plus escapes any `<` in message text so a
  pasted fragment can't be mistaken for a structural tag.
- Overwrites `_claude_timeline_auto` only, tagged with `source: "llm"` (vs. `"heuristic"`) so the UI
  can show which method produced a given tag (hover tooltip / "AI" vs. "auto" label). Never touches
  `_claude_timeline_user`.
- A one-time retry per batch on any failure; a network/API-level (systemic) failure aborts the whole
  run after the first batch rather than repeating an identical failure ~75 times; a
  malformed/miscounted individual result is applied positionally where possible rather than
  discarding the whole batch.
- **Custom in-page confirmation modal**, not `window.confirm()` — added after discovering that
  whatever restricted "modal preview" context the user was viewing the page through silently blocks
  *all* native browser dialogs (this also explained an earlier, separately-reported bug: the file
  `<input>` picker not opening at all). The custom modal is just page-owned DOM, so it works
  regardless of that sandboxing.
- **Progress bar turns red (`is-error` class) on failure** — this was a real bug (it stayed green
  regardless of outcome) that was reported and fixed.

---

## 6. Global Visibility Toggles — see §5.3 table above for full behavior.

## 7. Cross-View Navigation
**Requested, iteratively refined:**
- Clicking a flag or a session period must show the *whole session for context*, with just the
  relevant message(s) highlighted/scrolled-to — not filtered down to only the matching flag type
  (this was the original behavior and was explicitly corrected).
- Clicking anywhere should route through the Review tab (filtered appropriately), not open a modal —
  user explicitly rejected a modal-based design in favor of Review-tab filtering with a "Clear
  filter" affordance.
- Three navigable scopes, explicitly requested: **session/period**, **whole conversation**, **whole
  day** (all conversations active that day, grouped by conversation name then time — never
  interleaved chronologically across conversations). Day view is reachable both by expanding from a
  narrower filter and directly from the Calendar tab's date labels, with ◀ / ▶ arrows to step between
  adjacent days while in day view.

## 8. Showing Claude's Replies
**Requested:** a toggle to show Claude's replies alongside the user's messages for context, with the
explicit note that replies themselves are never flagged/annotated.

**Decisions:**
- No 1:1 reply pairing is assumed — the dedup work (§2.3) specifically established that a message can
  go unanswered (a resend implies the earlier attempt got no real reply), so a reply row only renders
  when one genuinely, immediately follows in the raw message sequence.
- Visual treatment: **tied to sender, not alternating by row position** (position-based striping was
  considered and explicitly rejected by the user in favor of sender-based tinting to avoid a "random
  stripe" look). Claude's reply rows reuse the existing paper-raised background color already used
  elsewhere on the page, with a small "Claude" label and slight indent, no checkboxes.

## 9. Persistence & Recovery
- **`window.storage`** (the Claude-artifact storage API) auto-saves/loads user overrides whenever
  available — this is the only persistence mechanism when running as a live artifact; a locally
  opened copy has no equivalent and falls back to manual export/import of the annotated file.
- **"Download annotated conversations.json"** is the single, unified save mechanism (superseding an
  earlier two-file design — separate "flags file" + "conversations file" — which was explicitly
  retired once flags were merged into the conversation file format, §2.4).
- **Incremental auto-save of LLM classification progress**, added after a real incident: a
  classification run that failed partway *and* then an export attempt that crashed the tab, losing
  everything and the tokens spent producing it. Now, classification results save to `window.storage`
  every 5 batches (and immediately on any early-abort failure), and are automatically recovered
  (merged in, without overwriting anything better already present) on the next file load — verified
  directly by simulating a mid-run crash after 10 successful batches and confirming a fresh reload
  recovered all 300 of those results.

---

## 10. Bugs Found & Fixed — Root Causes (kept for context; don't reintroduce these)

| Symptom | Root cause | Fix |
|---|---|---|
| Calendar bars ran past the edge of their day | Day-bucketing was computed server-side in UTC, then displayed in the viewer's local timezone, so a session's local-time position didn't always fall inside the UTC day it was filed under. | Moved all day-bucketing to run client-side, in the browser's own timezone, at render time. |
| Clicking a flag icon showed only one message, no context | Filter was narrowed to the specific flag type + exact message. | Filter widened to the whole session period; only the *highlight* stays specific to the flagged message(s). |
| Uncaught `TypeError: Cannot read properties of null` on clicking a calendar bar | A `requestAnimationFrame` callback closed over a mutable variable (`reviewHighlightIds`) that was nulled out synchronously *before* the deferred callback ran, so it read `null` instead of the intended array. | Captured the value in a local `const` before scheduling the callback. |
| ALL-CAPS false positives (IRS, DARPA, OK, etc.) | Naive regex with no acronym/real-word distinction. | Dictionary-based check (§5.1). |
| File `<input>` picker did nothing when clicked | The hosting "modal preview" context silently sandboxes native browser dialogs. | (Structural — no in-page fix possible; documented as a platform limitation, worked around for the *confirm* dialog by building a custom in-page modal, §5.4.) |
| `window.confirm()` before AI classification did nothing | Same native-dialog sandboxing as above. | Replaced with a custom, page-owned confirmation modal. |
| Classification error messages were unhelpful (generic "batch failed," logged only to console) | Errors were caught and only `console.error`'d, never shown in the UI; API-level error responses (e.g. bad model name, auth failure) weren't even inspected — only network-level and JSON-parse failures were. | Added explicit checks for `response.ok`/`data.error`, and surfaced the real error text in the visible status label. |
| "Expected 30, got 31" / model answering conversationally instead of classifying | Zero-shot prompt used plain numbered-list text with no strong delimiters; long batches of quoted user text could pull the model into continuing a "conversation" rather than treating it as data, and unusual message content could be misread as an extra list entry. | Rebuilt the prompt with explicit `<message index="N">` XML-style tags, an explicit stated count, and the "classify only, never respond" instruction repeated at both ends of the prompt; added tag-injection escaping. |
| Progress bar stayed green even when the run failed | No error-state styling existed. | Added `.is-error` class (red), applied on both early-abort and any-failures-at-completion paths. |
| Export crashed the page (white screen) | `exportAnnotatedConversations()` deep-cloned the *entire* raw conversation file via `JSON.parse(JSON.stringify(RAW_DATA))` before mutating it — for a 60+MB file, this briefly holds 3 full copies in memory at once. | Mutate `RAW_DATA` in place instead (safe here — only adding two namespaced fields, never removing anything); cuts peak memory roughly in half. |
| Classification work lost on any interruption before manual export | No persistence existed between "a batch completes" and "the user remembers to click download." | Incremental auto-save to `window.storage` during the run, with automatic recovery on next load (§9). |
| Claude's reply text displayed with all paragraph breaks/formatting collapsed | The reply-row CSS class never got the `white-space: pre-wrap` rule that the user's own message column had. | Went further than a CSS patch: built a small dependency-free markdown-lite renderer (headers, bold/italic, inline code, both list types, paragraph breaks) applied to both human messages and Claude replies, escaping raw text first so real HTML/script injection is still impossible (verified directly with a `<script>`/`<img onerror>` injection test). |

---

## 11. Known Limitations / Open Items (as of handoff)

- **"Classify with AI" only functions inside a live-rendered Claude artifact.** A downloaded,
  standalone copy of the HTML file can never reach the API from the browser — this is a CORS/server
  policy boundary, not something fixable client-side. Any future work here should assume this
  boundary is permanent unless the project adds a real backend proxy (which would mean leaving the
  "100% client-side, GitHub-Pages-able" architecture — a real fork in the road worth flagging to the
  user before undertaking).
- **Zero-shot LLM classification quality is unverified at scale** — was designed and unit/mock-tested
  for robustness (retries, partial-failure handling, prompt hardening), but real-world classification
  quality against the user's actual, much-larger manual corrections had not yet been evaluated at the
  point of this handoff. A natural next step (mentioned early on, then deferred): once zero-shot is
  working acceptably, consider surfacing genuinely ambiguous cases back to users for lightweight
  labeling to build a calibration set — explicitly scoped as future work, not yet started.
- **The Session-length-vs-flag-rate correlation came back ≈0 (r≈0.02)** on the original sample
  dataset using the heuristic detector. Whether this changes materially once real LLM-based
  classification is used at scale is an open empirical question, not yet re-tested end-to-end.
- **No automated test suite ships with the file** — all verification so far was done via ad hoc
  Node+jsdom scripts during development (mocking `fetch`, `window.storage`, and DOM events per
  feature), not a checked-in regression suite. Worth formalizing if development continues in Coder.
- **Format v2 is a breaking change** for any hypothetical external tool expecting the raw Anthropic
  export schema (root array vs. wrapped object). Accepted deliberately; flagged here so it isn't
  "rediscovered" as a regression later.
