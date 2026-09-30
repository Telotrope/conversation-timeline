# Split timeline.html's script into modules

**Status:** proposal, awaiting review. Two decisions in [Open questions (line 168)](2026-09-30-split-timeline-script.md#L168) must be
answered before coding.

## Goal

[timeline.html](../../timeline.html) has one `<script>` block, lines 909–3107 (about 2,200 lines),
holding 85 top-level functions plus shared state and event wiring. Split it into small files
with one concern each, grouped by layer the way the Rust backend is split into
`timeline-core` (domain), `timeline-storage` (adapters) and `timeline-api` (wiring).

This is a **move, not a rewrite**. Function bodies move unchanged except for the four kinds of
edits listed in §3, each of which exists only to make the split legal.

Out of scope: the CSS and markup in lines 1–908; any behavior change; retiring the
Claude-artifact-only AI classification feature.

## 1. Layers

Terms used below:
- **Module**: a JavaScript file that declares what it imports and exports, instead of sharing
  one global namespace with every other script on the page.
- **DOM** (Document Object Model): the browser's live, editable model of the page.
- **Layer**: a group of modules allowed to import only from layers below it. "Below" means
  more general and less tied to the browser.

```
main.js            wiring: event listeners, startup, "refresh all views"
  └─ app/          flows that coordinate several views + the network
      └─ ui/       reads and writes the DOM
          └─ infra/   network and browser storage (no DOM)
              └─ core/    pure data logic (no DOM, no network, no storage)
```

`core/` may import nothing outside `core/`. This mirrors the backend rule that domain code
never imports infrastructure, and it means every `core/` module can be unit-tested in Node
without a browser.

New directory: `frontend/` at the repo root. [timeline.html](../../timeline.html) stays where it is so
the [README.md](../../README.md), [.vscode/tasks.json](../../.vscode/tasks.json) and
[scripts/test-dev-up.sh](../../scripts/test-dev-up.sh) links keep working.

## 2. Inventory

Line numbers are current positions in [timeline.html](../../timeline.html). Sizes are approximate.

### core/ — pure data logic

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `core/export-format.js` | `FORMAT_VERSION`, `extractMessageText`, `unwrapUploadedJSON`, `parseUploadedConversations` | [939–1025](../../timeline.html#L939) | 90 |
| `core/state.js` | The shared data: conversations, raw data, messages, human messages, human-by-id map, blocks, overrides, the three show-auto/user/replies toggles, `GAP_THRESHOLD_SEC` | [1449–1468](../../timeline.html#L1449), [1495](../../timeline.html#L1495) | 50 |
| `core/flags.js` | `hasUserValue`, `effectiveFlag`, `isOverridden`, `isFlagged`, `attachFlags` | [1470–1503](../../timeline.html#L1470), [1553–1578](../../timeline.html#L1553), [2659](../../timeline.html#L2659) | 70 |
| `core/blocks.js` | `localDateKey`, `buildBlocks` (splits activity into sessions separated by 15+ minute idle gaps) | [1504–1552](../../timeline.html#L1504) | 50 |
| `core/format.js` | Human-readable formatting: `formatBytes`, `formatEta`, `fmtDuration`, `fmtClock`, `fmtDayHeading`, `fmtMonthHeading` | [1134–1154](../../timeline.html#L1134), [1955–1975](../../timeline.html#L1955) | 45 |
| `core/analyses.js` | `pearsonR`, `computeWithProgress`, and the five `compute…Analysis` functions (friction, trend, length, time of day, idle gap) | [2663–2711](../../timeline.html#L2663), plus the compute half of each section in [2751–2994](../../timeline.html#L2751) | 200 |
| `core/classify-prompt.js` | `CLASSIFY_BATCH_SIZE`, `CLASSIFY_MODEL`, `getPriorReplyText`, `escapeForPromptTags`, `buildClassifyPrompt` | [1690–1740](../../timeline.html#L1690) | 55 |

### infra/ — network and storage, no DOM

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `infra/api-client.js` | `resolveApiBase`, `API_BASE`, the auth token, `ensureAuthToken`, `describeFailure`, `putWithProgress`, `readBodyWithProgress`, `patchFlagsToBackend` | [1045–1099](../../timeline.html#L1045), [1174–1216](../../timeline.html#L1174), [1624–1647](../../timeline.html#L1624) | 140 |
| `infra/anthropic-classifier.js` | `classifyBatchWithAI`, `classifyBatchWithRetry` (the direct call to Anthropic's API, which only works when the page runs as a Claude artifact) | [1741–1808](../../timeline.html#L1741) | 70 |
| `infra/artifact-storage.js` | `AUTO_CACHE_KEY`, `hasStorage`, `saveAutoClassificationsToStorage` | [1580–1623](../../timeline.html#L1580) | 40 |

### ui/ — reads and writes the page

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `ui/confirm-modal.js` | `showConfirm` (the page's own replacement for the browser's confirm dialog) | [912–938](../../timeline.html#L912) | 25 |
| `ui/status-indicators.js` | Load-screen status and progress bar: `setLoadStatus`, `showLoadProgress`, `hideLoadProgress`, `failLoadProgress`, `setLoadProgressIndeterminate`, `makeRateEstimator`, `showRestoredNotice`; and the save indicator, `setSaveStatus` | [1026–1044](../../timeline.html#L1026), [1100–1133](../../timeline.html#L1100), [1155–1173](../../timeline.html#L1155), [1290–1302](../../timeline.html#L1290), [1648–1657](../../timeline.html#L1648) | 100 |
| `ui/markup.js` | `escapeHtml`, `renderMarkdownLite` | [2196–2281](../../timeline.html#L2196) | 90 |
| `ui/page-chrome.js` | `switchTab`, `renderSubtitle` | [2282–2291](../../timeline.html#L2282), [1976–1983](../../timeline.html#L1976) | 30 |
| `ui/location.js` | Writing the URL hash: `currentLocationHash`, `rememberLocation`, the `APPLYING_HASH` guard | [2292–2326](../../timeline.html#L2292) | 40 |
| `ui/calendar.js` | `PALETTE`, `colorFor`, `renderCalendar` | [1952–1953](../../timeline.html#L1952), [1984–2079](../../timeline.html#L1984) | 100 |
| `ui/conversations.js` | `renderConvList`, `currentConv`, `selectConversation` | [2080–2195](../../timeline.html#L2080) | 120 |
| `ui/review.js` | `PAGE_SIZE`, the review filter state, `getFilteredHumanMessages`, `jumpToReview`, `jumpToReviewDay`, `shiftReviewDay`, `clearReviewFilters`, `checkboxCell`, `renderReviewFilterBanner`, `renderReplyRow`, `renderReviewTable` | [2356–2629](../../timeline.html#L2356) | 275 |
| `ui/charts.js` | `renderBarChartSVG`, `renderLineChartSVG`, `renderScatterChartSVG` | [2995–3105](../../timeline.html#L2995) | 110 |
| `ui/analytics.js` | `ANALYTICS_META`, `currentAnalysis`, `analysisCache`, `showAnalyticsProgress`, `setAnalyticsProgress`, `runAnalysis`, and the five `render…Result` functions | [2701–2749](../../timeline.html#L2701), plus the render half of each section in [2786–2994](../../timeline.html#L2786) | 230 |

### app/ — flows spanning several views and the network

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `app/load-flow.js` | `applyExportText`, `tryRestoreSession`, `runDetectionPass`, `handleLoadClick` | [1217–1289](../../timeline.html#L1217), [1303–1434](../../timeline.html#L1303) | 210 |
| `app/flag-edits.js` | `setRowOverrides`, `approveRow`, `onVisibilityToggleChanged` | [1658–1684](../../timeline.html#L1658), [2635–2643](../../timeline.html#L2635) | 45 |
| `app/classify-run.js` | `classifyWithAI` | [1809–1901](../../timeline.html#L1809) | 95 |
| `app/annotated-export.js` | `exportAnnotatedConversations` | [1902–1951](../../timeline.html#L1902) | 50 |
| `app/router.js` | `applyLocationHash` (reads the URL hash and opens the right tab, conversation or analysis) | [2327–2348](../../timeline.html#L2327) | 30 |

### Top level

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `main.js` | Every top-level `addEventListener` call, the `refreshAllViews` listener (§3c), and the startup call to `tryRestoreSession` | [1435–1447](../../timeline.html#L1435), [2349–2354](../../timeline.html#L2349), [2630–2653](../../timeline.html#L2630), [2746–2749](../../timeline.html#L2746), [3106](../../timeline.html#L3106) | 60 |

Largest file: `ui/review.js` at about 275 lines, under the 300–400 soft target.

## 3. The edits the split forces

**3a. Shared state becomes one object.** An imported variable cannot be reassigned by the
importing module, but [applyExportText (line 1221)](../../timeline.html#L1221) reassigns
`CONVERSATIONS`, `MESSAGES`, `BLOCKS` and four others. `core/state.js` exports one `state` object
and every reference changes from `CONVERSATIONS` to `state.conversations`. This is the largest
mechanical edit, touching every module that reads the data.

**3b. Infra functions stop touching the page.**
- `ensureAuthToken` reads the login box directly ([line 1063](../../timeline.html#L1063)). It
  will take the login name as a parameter; `app/load-flow.js` reads the box.
- `patchFlagsToBackend` calls `setSaveStatus` four times
  ([lines 1626–1644](../../timeline.html#L1626)). It will return a result message instead;
  `app/flag-edits.js` shows it.

**3c. Two circular dependencies are broken with page events.** A circular dependency is two
files each needing the other, so neither can be understood or tested alone. The project rules
forbid them.
- *Flag edits ↔ views:* the review table's checkboxes call `setRowOverrides`, which then
  redraws the calendar, conversation list and review table. Instead, the review table fires a
  `timeline:flag-toggled` page event; `main.js` routes it to `setRowOverrides`. After any data
  change, `app/` modules fire `timeline:data-changed`, and one `refreshAllViews` handler in
  `main.js` redraws everything. That handler replaces the redraw sequence now repeated in
  `applyExportText`, `setRowOverrides`, `classifyWithAI` and `onVisibilityToggleChanged`.
- *URL hash ↔ views:* views record their position in the hash, and reading the hash opens
  views. Writing the hash (`ui/location.js`) and reading it (`app/router.js`) go in separate
  files, so views depend only on the writer and the reader depends on views.

Page events are built into the browser (`CustomEvent`), so no library is needed.

**3d. How the page loads the files.** This depends on Open question 1.

## 4. Tests and checks

- **Existing end-to-end tests are the regression guard.** Those are the Playwright tests in
  [e2e/views.spec.js](../../e2e/views.spec.js) and
  [e2e/upload-flow.spec.js](../../e2e/upload-flow.spec.js), which drive the real page against the
  real backend. They run after every extraction commit.
- **New unit tests for `core/`** use Node's built-in test runner (`node:test`, part of Node, no
  new dependency). They cover every exported function, and line coverage is measured with
  `node --test --experimental-test-coverage`.
- **Coverage for `ui/`, `app/`, `infra/` is measured, not assumed.** Playwright's Chromium
  coverage API reports which lines the end-to-end run executes. I'll report the gaps and propose
  tests for them rather than claim 100%.
- **New structural checks**, in the same style as the backend's ratchet tests:
  - `madge --circular` (MIT license) fails on any circular import.
  - A file-size check fails on any `frontend/` file over 1,000 lines.
  - A layer check fails if `core/` mentions `document`, `window`, `fetch` or `localStorage`, or
    imports from outside `core/`.

These live in a new `frontend/package.json` with `madge` as its only dependency.

## 5. Order of work

One commit per step, with the end-to-end suite green before each commit:

1. Set up module loading (per Open question 1), and add `main.js` holding the whole script
   unchanged. This proves the loading path before anything moves.
2. Extract `core/` modules and add their unit tests. Introduce `state` (§3a) here.
3. Extract `infra/`, with the §3b signature changes.
4. Extract `ui/` leaf modules: confirm modal, status indicators, markup, page chrome, location,
   charts.
5. Extract the views: calendar, conversations, review, analytics. Introduce the page events
   (§3c).
6. Extract `app/` and `app/router.js`. `main.js` shrinks to wiring.
7. Add the structural checks and the coverage measurement, and report the coverage numbers.

## Open questions

**Q1. How should the page load the files?** Browsers refuse to load modules into a page opened
from disk (`file://`), and both end-to-end specs open it that way
([e2e/views.spec.js:18](../../e2e/views.spec.js#L18)). The [README.md](../../README.md) already tells
users to serve it over HTTP instead.
- **(A, recommended) Real modules, served over HTTP.** No build step, and the imports are
  readable in the browser's developer tools. This requires changing the `TIMELINE_HTML` line in
  both specs to use the static server, which is a test change that needs your approval.
- **(B) Modules in source, bundled into timeline.html by esbuild (MIT license).** Keeps
  `file://` and a single self-contained file, but adds a build step. timeline.html becomes a
  generated file, and forgetting to rebuild ships stale code.
- **(C) Plain `<script src>` files without modules.** Works from disk with no build, but
  everything stays in one shared global namespace. Nothing enforces the layers, and `madge`
  cannot see the dependencies. This splits the text without the structure; not recommended.

**Q2. Does the page still need to run as a single-file Claude artifact?** The AI
classification feature only works there ([line 1685](../../timeline.html#L1685)). Option A
breaks single-file hosting; option B preserves it. If artifact hosting is already dead (the
comment at [line 1599](../../timeline.html#L1599) says V3 replaces the feature), A is the simpler
choice.

## Self-critique log

### C1 [OPEN]: The loading approach decides whether the end-to-end tests change
Modules do not load from `file://`, which is how both specs open the page. **Mitigation in
plan:** three options laid out with a recommendation in [Q1 (line 170)](2026-09-30-split-timeline-script.md#L170). **Open:** needs
your answer; this triggers step 1.

### C2 [OPEN]: Splitting may break single-file artifact hosting
**Mitigation in plan:** option B keeps it; see [Q2 (line 184)](2026-09-30-split-timeline-script.md#L184). **Open:** needs your
answer on whether artifact hosting still matters.

### C3 [RESOLVED]: A naive split creates circular imports
Original concern: I read in the call map that the review table calls `setRowOverrides`, which
calls `renderReviewTable`. Likewise, views call `rememberLocation` while `applyLocationHash`
calls the views.
**Resolution:** page events plus a separate writer and reader for the URL hash;
[§3c (line 116)](2026-09-30-split-timeline-script.md#L116).

### C4 [RESOLVED]: Imported variables cannot be reassigned
Original concern: `applyExportText` reassigns seven shared globals, which modules forbid.
**Resolution:** a single `state` object; [§3a (line 103)](2026-09-30-split-timeline-script.md#L103).

### C5 [RESOLVED]: Infra functions reached into the page
Original concern: `ensureAuthToken` reads a text box and `patchFlagsToBackend` writes the save
indicator, which would make `infra/` depend on the DOM.
**Resolution:** a parameter and a return value respectively; [§3b (line 109)](2026-09-30-split-timeline-script.md#L109).

### C6 [RESOLVED]: The first draft had about 29 files, several under 15 lines
**Resolution:** save status merged into `ui/status-indicators.js`; `renderSubtitle` joined
`switchTab` in `ui/page-chrome.js`; `pearsonR` joined `core/analyses.js`. That leaves 26 files.

### C7 [OPEN]: 100% coverage of the browser-side layers is not yet demonstrated
The project rule requires it, and I have not measured what the end-to-end suite covers today.
**Mitigation in plan:** the measurement is step 7 of [§5 (line 153)](2026-09-30-split-timeline-script.md#L153), reported rather than
assumed. **Open:** if the measurement shows gaps, I'll propose specific added end-to-end tests
for your approval.

### C8 [OPEN]: Line ranges were located by pattern matching, not by reading every function
I built the inventory from function declarations, section banners, and keyword counts for DOM,
network and storage use. The two §3b cases were confirmed by reading the lines. Top-level
statements between functions (constants, the `let` state) may belong to a different file than
listed. **Open:** each extraction commit reads the full range it moves. Any misplacement gets
reported in that step's commit message.
