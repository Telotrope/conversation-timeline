# Split timeline.html's script into modules

**Status:** revision 2, awaiting review. Revision 1's open questions are answered: the page
is served only over HTTP, and nothing needs to preserve single-file hosting.

## Goal

[timeline.html](../../timeline.html) has one `<script>` block, lines 909–3107 (about 2,200 lines),
holding 85 top-level functions plus shared state and event wiring. Split it into small files
with one concern each, grouped by layer the way the Rust backend is split into
`timeline-core` (domain), `timeline-storage` (adapters) and `timeline-api` (wiring).

This is a **move, not a rewrite**. Function bodies move unchanged except for the edits listed
in §3 and §4. Each of those edits exists only to make the split legal.

**Serving.** The page is served only over HTTP. The script becomes standard JavaScript modules
loaded with `<script type="module" src="frontend/main.js">`. There is no build step and no
bundler, and nothing is done to keep the page working when opened from disk.

Out of scope:
- The CSS and markup in lines 1–908.
- Any behavior change.
- Replacing the URL-hash navigation with `history.pushState`. The comment at
  [timeline.html:2298](../../timeline.html#L2298) justifies hashes by `file://` use, which no longer
  applies. Switching would change behavior, so that is a separate decision. The comment moves
  with the code and is updated to say the reason no longer holds.
- The "Classify with AI" feature. It calls `api.anthropic.com` directly from the browser
  ([timeline.html:1745](../../timeline.html#L1745)), which cannot succeed from a page served over
  HTTP. It moves unchanged. Whether to delete it is a separate decision.

## 1. Layers

Terms used below:
- **Module**: a JavaScript file that declares what it imports and exports, instead of sharing
  one global namespace with every other script on the page.
- **Import**: one module naming another module it depends on. The browser loads imports
  before running the importing module.
- **DOM** (Document Object Model): the browser's live, editable model of the page.
- **Layer**: a group of modules allowed to import only from layers below it. "Below" means
  more general and less tied to the browser.

```
main.js            wiring: event listeners, startup
  └─ app/          flows that coordinate several views + the network
      └─ ui/       reads and writes the DOM
          └─ infra/   network and browser storage (no DOM)
              └─ core/    pure data logic (no DOM, no network, no storage)
```

`core/` may import nothing outside `core/`. This mirrors the backend rule that domain code
never imports infrastructure. It also means every `core/` module can be unit-tested in Node
without a browser.

New directory: `frontend/` at the repo root. [timeline.html](../../timeline.html) stays where it is so
the [README.md](../../README.md), [.vscode/tasks.json](../../.vscode/tasks.json) and
[scripts/test-dev-up.sh](../../scripts/test-dev-up.sh) references keep working.

## 2. Inventory

Line numbers are current positions in [timeline.html](../../timeline.html). Sizes are approximate.

### core/ — pure data logic

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `core/export-format.js` | `FORMAT_VERSION`, `extractMessageText`, `unwrapUploadedJSON`, `parseUploadedConversations` | [939–1025](../../timeline.html#L939) | 90 |
| `core/state.js` | The shared data: conversations, raw data, messages, human messages, human-by-id map, blocks, overrides, the three show-auto/user/replies toggles, `GAP_THRESHOLD_SEC`. Also the two "where you are" values that today live with their views: the open conversation (`currentConv`, [line 2109](../../timeline.html#L2109)) and the shown analysis (`currentAnalysis`, [line 2709](../../timeline.html#L2709)). §4 explains why they move here. | [1449–1468](../../timeline.html#L1449), [1495](../../timeline.html#L1495) | 55 |
| `core/flags.js` | `hasUserValue`, `effectiveFlag`, `isOverridden`, `isFlagged`, `attachFlags` | [1470–1503](../../timeline.html#L1470), [1553–1578](../../timeline.html#L1553), [2659](../../timeline.html#L2659) | 70 |
| `core/blocks.js` | `localDateKey`, `buildBlocks` (splits activity into sessions separated by 15+ minute idle gaps) | [1504–1552](../../timeline.html#L1504) | 50 |
| `core/format.js` | Human-readable formatting: `formatBytes`, `formatEta`, `fmtDuration`, `fmtClock`, `fmtDayHeading`, `fmtMonthHeading` | [1134–1154](../../timeline.html#L1134), [1955–1975](../../timeline.html#L1955) | 45 |
| `core/analyses.js` | `pearsonR`, `computeWithProgress`, and the five `compute…Analysis` functions (friction, trend, length, time of day, idle gap) | [2663–2700](../../timeline.html#L2663), plus the compute half of each section in [2751–2994](../../timeline.html#L2751) | 200 |
| `core/classify-prompt.js` | `CLASSIFY_BATCH_SIZE`, `CLASSIFY_MODEL`, `getPriorReplyText`, `escapeForPromptTags`, `buildClassifyPrompt` | [1690–1740](../../timeline.html#L1690) | 55 |

### infra/ — network and storage, no DOM

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `infra/api-client.js` | `resolveApiBase`, `API_BASE`, the auth token, `ensureAuthToken`, `describeFailure`, `putWithProgress`, `readBodyWithProgress`, `patchFlagsToBackend` | [1045–1099](../../timeline.html#L1045), [1174–1216](../../timeline.html#L1174), [1624–1647](../../timeline.html#L1624) | 140 |
| `infra/anthropic-classifier.js` | `classifyBatchWithAI`, `classifyBatchWithRetry` | [1741–1808](../../timeline.html#L1741) | 70 |
| `infra/artifact-storage.js` | `AUTO_CACHE_KEY`, `hasStorage`, `saveAutoClassificationsToStorage` | [1580–1623](../../timeline.html#L1580) | 40 |

### ui/ — reads and writes the page

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `ui/confirm-modal.js` | `showConfirm` (the page's own replacement for the browser's confirm dialog) | [912–938](../../timeline.html#L912) | 25 |
| `ui/status-indicators.js` | Load-screen status and progress bar: `setLoadStatus`, `showLoadProgress`, `hideLoadProgress`, `failLoadProgress`, `setLoadProgressIndeterminate`, `makeRateEstimator`, `showRestoredNotice`; and the save indicator, `setSaveStatus` | [1026–1044](../../timeline.html#L1026), [1100–1133](../../timeline.html#L1100), [1155–1173](../../timeline.html#L1155), [1290–1302](../../timeline.html#L1290), [1648–1657](../../timeline.html#L1648) | 100 |
| `ui/markup.js` | `escapeHtml`, `renderMarkdownLite` | [2196–2281](../../timeline.html#L2196) | 90 |
| `ui/page-chrome.js` | `switchTab`, `renderSubtitle` | [2283–2290](../../timeline.html#L2283), [1976–1983](../../timeline.html#L1976) | 30 |
| `ui/location.js` | Writing the URL hash: `currentLocationHash`, `rememberLocation`, and the guard that stops the hash being rewritten while it is being applied | [2292–2326](../../timeline.html#L2292) | 40 |
| `ui/calendar.js` | `PALETTE`, `colorFor`, `renderCalendar` | [1952–1953](../../timeline.html#L1952), [1984–2079](../../timeline.html#L1984) | 100 |
| `ui/conversations.js` | `renderConvList`, `selectConversation` | [2080–2195](../../timeline.html#L2080) | 115 |
| `ui/review.js` | `PAGE_SIZE`, the review filter state, `getFilteredHumanMessages`, `jumpToReview`, `jumpToReviewDay`, `shiftReviewDay`, `clearReviewFilters`, `checkboxCell`, `renderReviewFilterBanner`, `renderReplyRow`, `renderReviewTable`, and the one-time setup of the flag-edit handlers (§4, loop 1) | [2356–2629](../../timeline.html#L2356) | 285 |
| `ui/charts.js` | `renderBarChartSVG`, `renderLineChartSVG`, `renderScatterChartSVG` | [2995–3105](../../timeline.html#L2995) | 110 |
| `ui/analytics.js` | `ANALYTICS_META`, `analysisCache`, `showAnalyticsProgress`, `setAnalyticsProgress`, `runAnalysis`, and the five `render…Result` functions | [2701–2749](../../timeline.html#L2701), plus the render half of each section in [2786–2994](../../timeline.html#L2786) | 225 |

### app/ — flows spanning several views and the network

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `app/load-flow.js` | `applyExportText`, `tryRestoreSession`, `runDetectionPass`, `handleLoadClick` | [1217–1289](../../timeline.html#L1217), [1303–1434](../../timeline.html#L1303) | 210 |
| `app/refresh-views.js` | `refreshAllViews`: the "recompute flags, then redraw the calendar, conversation list, open conversation and review table" sequence now copied three times (§4, loop 1) | [1667–1672](../../timeline.html#L1667), [1889–1893](../../timeline.html#L1889), [2638–2642](../../timeline.html#L2638) | 15 |
| `app/flag-edits.js` | `setRowOverrides`, `approveRow`, `onVisibilityToggleChanged` | [1658–1684](../../timeline.html#L1658), [2635–2643](../../timeline.html#L2635) | 45 |
| `app/classify-run.js` | `classifyWithAI` | [1809–1901](../../timeline.html#L1809) | 95 |
| `app/annotated-export.js` | `exportAnnotatedConversations` | [1902–1951](../../timeline.html#L1902) | 50 |
| `app/router.js` | `applyLocationHash` (reads the URL hash and opens the right tab, conversation or analysis) | [2327–2348](../../timeline.html#L2327) | 30 |

### Top level

| File | Contents | Lines now | ~Size |
|---|---|---|---|
| `main.js` | Every top-level `addEventListener` call, handing the flag-edit handlers to the review table (§4, loop 1), and the startup call to `tryRestoreSession` | [1435–1447](../../timeline.html#L1435), [2349–2354](../../timeline.html#L2349), [2630–2653](../../timeline.html#L2630), [2746–2749](../../timeline.html#L2746), [3106](../../timeline.html#L3106) | 60 |

That makes 27 files. The largest is `ui/review.js` at about 285 lines, under the 300–400 soft
target.

## 3. Mechanical edits the split forces

**3a. Shared state becomes one object.** A module cannot reassign a variable it imported. But
[applyExportText (line 1221)](../../timeline.html#L1221) reassigns `CONVERSATIONS`, `MESSAGES`,
`BLOCKS` and four others. So `core/state.js` exports one `state` object, and every reference
changes from `CONVERSATIONS` to `state.conversations`. This is the largest mechanical edit and
touches every module that reads the data.

**3b. Infra functions stop touching the page.**
- `ensureAuthToken` reads the login box directly ([line 1063](../../timeline.html#L1063)). It
  will take the login name as a parameter, and `app/load-flow.js` reads the box.
- `patchFlagsToBackend` calls `setSaveStatus` four times
  ([lines 1626–1644](../../timeline.html#L1626)). It will return the message instead, and
  `app/flag-edits.js` shows it.

**3c. Cross-file names are imported and exported.** Every function another file calls gets
`export` in front of it, and the calling file gets an `import` line. The only other change to
function bodies is the `state.` prefix from 3a.

## 4. How the loops between files are resolved

A **circular dependency** is two or more files that import each other, directly or through a
chain. The project rules forbid them. If today's functions were simply cut into the files
above, there would be three loops. Each one is listed below with the call sites I read that
create it, and how it is removed.

A runtime call loop is different from an import loop. Clicking a checkbox in the review table
makes the review table redraw itself, and that stays true after the split. What goes away is
two *files* each naming the other in their imports. The fix is to pass the upward call in as a
function, set up once by `main.js`. Nothing changes about what happens when you click.

### Loop 1: review table → flag edits → review table (and the other views)

Today:
- The review table's checkboxes and Approve buttons call `setRowOverrides` and `approveRow`
  ([lines 2595–2603](../../timeline.html#L2595)).
- `setRowOverrides` redraws the calendar, the conversation list, the open conversation and the
  review table ([lines 1667–1672](../../timeline.html#L1667)).
- `selectConversation` calls `jumpToReview` ([lines 2165–2192](../../timeline.html#L2165)).

Cut naively, `ui/review.js` imports `app/flag-edits.js`, which imports `ui/review.js` and
`ui/conversations.js`, which imports `ui/review.js`. That is a loop. It also breaks the layer
rule, because a `ui/` file would import an `app/` file.

Fix: `ui/review.js` does not import the flag-edit functions. It exports a setup function that
receives them:

```js
// ui/review.js
let onFlagToggled = null;   // (id, type, checked) => void
let onRowApproved = null;   // (id) => void
export function setFlagEditHandlers(toggle, approve){
  onFlagToggled = toggle;
  onRowApproved = approve;
}
// ...inside renderReviewTable, where setRowOverrides / approveRow are called today:
cb.addEventListener('change', (e)=> onFlagToggled(e.target.dataset.id, e.target.dataset.type, e.target.checked));
btn.addEventListener('click', ()=> onRowApproved(btn.dataset.id));

// main.js
import { setFlagEditHandlers } from './ui/review.js';
import { setRowOverrides, approveRow } from './app/flag-edits.js';
setFlagEditHandlers(setRowOverrides, approveRow);
```

The resulting import chains all point downward:
- `main.js` → `app/flag-edits.js` → `app/refresh-views.js` → `ui/review.js`
- `main.js` → `ui/review.js`

`ui/review.js` imports nothing from `app/`.

I chose passing functions in over firing named page events (what revision 1 proposed). The
reason is readability, which prompted this plan. A reader who searches for
`setFlagEditHandlers` finds the single line in `main.js` that says exactly which function runs.
An event named by a string can have any number of listeners anywhere, found only by searching
for the string.

The same redraw sequence is copied in `setRowOverrides`, `classifyWithAI`
([lines 1889–1893](../../timeline.html#L1889)) and `onVisibilityToggleChanged`
([lines 2638–2642](../../timeline.html#L2638)). It becomes one function, `refreshAllViews`, in
`app/refresh-views.js`, and all three call it. `applyExportText`'s redraw
([lines 1244–1247](../../timeline.html#L1244)) is different: it also redraws the subtitle and
does not reopen a conversation. It stays as it is.

### Loop 2: views → URL writer → views

Today:
- `selectConversation` and `runAnalysis` call `rememberLocation`
  ([line 2112](../../timeline.html#L2112), [line 2729](../../timeline.html#L2729)) to record where you
  are in the URL hash.
- `rememberLocation` calls `currentLocationHash`, which reads `currentConv` and
  `currentAnalysis` ([lines 2311–2319](../../timeline.html#L2311)). Those variables are declared
  beside `selectConversation` and `runAnalysis`.

Cut naively, `ui/conversations.js` imports `ui/location.js` for `rememberLocation`, and
`ui/location.js` imports `ui/conversations.js` for `currentConv`. The same is true for
analytics. Revision 1 said separating the URL writer from the reader fixed this. That was
wrong, because the writer alone still reads the views' variables.

Fix: the two "where you are" values move into `core/state.js`, as
`state.selectedConversation` and `state.selectedAnalysis`. `selectConversation` and
`runAnalysis` write them there, and `ui/location.js` reads them from there. Now
`ui/location.js` imports only `core/state.js`. The views import `ui/location.js`, and nothing
imports back.

### Loop 3: URL reader → views → URL reader

Today, `applyLocationHash` ([lines 2327–2348](../../timeline.html#L2327)) calls `switchTab`,
`selectConversation` and `runAnalysis`. It also sets the `APPLYING_HASH` guard that
`rememberLocation` checks, so that opening a view from the URL does not immediately write the
URL back.

If `applyLocationHash` shared a file with `rememberLocation`, that file would import the views,
and the views would import it: a loop. Fix: the reader goes in `app/router.js`, above the
views, and the writer stays in `ui/location.js`, below them. The guard stays private to
`ui/location.js`, which exports one function for the router to use:

```js
// ui/location.js
let applyingHash = false;
export function whileApplyingHash(fn){
  applyingHash = true;
  try { fn(); } finally { applyingHash = false; }
}
```

This is the same try/finally that `applyLocationHash` has today, moved behind a function so no
other file can set the flag and forget to clear it.

### The resulting graph

These are the imports that cross between files on the same layer, or reach up from one layer
to another. Plain downward imports, for example anything importing `core/state.js`, are left
out. Each line reads "this file imports these":

| File | Imports (beyond `core/` and `infra/`) |
|---|---|
| `main.js` | every `app/` file, `ui/review.js` (to call `setFlagEditHandlers`), and each view for its listeners |
| `app/router.js` | `ui/page-chrome.js`, `ui/conversations.js`, `ui/analytics.js`, `ui/location.js` |
| `app/flag-edits.js` | `app/refresh-views.js`, `ui/status-indicators.js` |
| `app/classify-run.js` | `app/refresh-views.js`, `ui/confirm-modal.js` |
| `app/refresh-views.js` | `ui/calendar.js`, `ui/conversations.js`, `ui/review.js` |
| `ui/calendar.js` | `ui/review.js`, `ui/markup.js` |
| `ui/conversations.js` | `ui/review.js`, `ui/location.js`, `ui/markup.js` |
| `ui/analytics.js` | `ui/review.js`, `ui/location.js`, `ui/charts.js`, `ui/markup.js` |
| `ui/review.js` | `ui/page-chrome.js`, `ui/markup.js` |
| `ui/location.js` | nothing outside `core/` |

Reading down the table, no file imports anything that imports it back. `ui/review.js` is
imported by three views and imports none of them. I derived this table from the call sites
quoted in §4 and the call map, not from running a tool. The check for circular imports
(§5, step V7) runs on every test run and would catch a mistake in it.

## 5. How we verify the program still works

The claim to verify: after the split, the page behaves the same as before for every feature
reachable over HTTP. Three kinds of evidence support it:
- Browser tests that pass against the old page and then pass unchanged against the new one.
- Coverage numbers, measured before and after.
- Checks on the structure of the code.

Terms:
- **End-to-end test**: a Playwright test that drives the real page in a real Chrome against the
  real backend. The existing ones are in [e2e/views.spec.js](../../e2e/views.spec.js) and
  [e2e/upload-flow.spec.js](../../e2e/upload-flow.spec.js).
- **Characterization test**: a test written to record what the code does today, whether or not
  that behavior is ideal, so that any change in it fails the test.
- **Coverage**: which lines of the script actually ran during a test run.

### Before any code moves (on today's single-file page)

**V1. Serve the page over HTTP in the tests.** Both specs open the page from disk today
([e2e/views.spec.js:18](../../e2e/views.spec.js#L18), [e2e/upload-flow.spec.js:13](../../e2e/upload-flow.spec.js#L13)).
- `TIMELINE_HTML` changes to an `http://127.0.0.1:<port>/timeline.html` address.
- The specs start `python3 -m http.server` from the repo root the same way they already start
  the backend. They wait for the port, then stop the server afterward.
- [e2e/README.md](../../e2e/README.md) gets updated to match.

This is the test change your answer #1 authorizes. The suite must pass on the unchanged page
before anything else happens.

**V2. Fail on any uncaught browser error.** A misspelled import or a missing export stops a
module from loading. The page then stops working with only a message in the browser console,
and some tests could still pass against the blank parts of the page.
- Each spec registers `page.on('pageerror')` and fails the test if anything fires.
- This adds a check to the existing test files without changing any existing assertion. It
  needs your approval as part of approving this plan.

**V3. Measure baseline coverage.** Run the whole suite with Playwright's Chromium coverage
collection on the single-file page. Record which of the 85 functions never run, and the line
coverage within those that do. Write the result to
`docs/analysis/2026-09-30-timeline-script-baseline-coverage.md`. I have not measured this yet;
the list below comes from searching the test files for element names, not from running
coverage.

**V4. Add characterization tests for the paths the split rewires.** These must exist and pass
on the unchanged page before step S1. Judging by a keyword search of the specs, these look
untested:
- Ticking a flag checkbox in the review table. Asserts the calendar day, the conversation
  list, the open conversation and the table all show the change. This exercises loop 1.
- The show-automatic and show-mine toggles. Asserts all four views redraw. Also the
  show-replies toggle, which asserts replies appear in the review table.
- Clicking a friction-ranking row, a calendar session bar, and the "review this conversation"
  control in a conversation. Asserts each lands in the review tab with the right filter. This
  exercises the imports into `ui/review.js`.
- Opening `#analytics/<name>` directly and opening `#conversations/<n>` directly. Asserts the
  right view opens and the hash is not rewritten on load. This exercises loops 2 and 3 and the
  guard.
- The review filter banner's buttons: previous day, next day, clear, view the whole
  conversation, view the whole day.
- The "Classify with AI" button. Asserts the confirmation dialog appears and that Cancel leaves
  every flag unchanged. The success path cannot run over HTTP (see Out of scope), so it is not
  verified. §7 says so.

V3's measurement is the authority. Any function it shows never running gets a test here, or a
written reason in the analysis doc why no test can reach it over HTTP.

### During the split

**V5. After each step, before each commit:**
- The full end-to-end suite passes, including V4, with V2's error check active.
- The `core/` unit tests pass.
- No test is edited to make it pass. If a test fails, the code is wrong until I show you
  otherwise.

**V6. Each commit shows that code moved rather than changed.** I run `git diff
--color-moved=dimmed-zebra --stat` on each commit. It marks lines that moved between files
separately from lines that changed. The commit message lists every changed line group, and each
one must be one of the edits in §3 or §4. Anything else is a mistake to undo, not to explain.

### After the split

**V7. Structural checks**, added as tests so they keep running:
- `madge --circular frontend/` (MIT license) fails on any circular import. This is the check
  that proves §4 worked.
- A file-size check fails on any `frontend/` file over 1,000 lines.
- A layer check fails if a `core/` file mentions `document`, `window`, `fetch` or
  `localStorage`, or imports from outside `core/`. It also fails if a `ui/` file imports from
  `app/`, or an `infra/` file mentions `document`.

These live in a new `frontend/package.json`, with `madge` as its only dependency.

**V8. Coverage after vs. before.** Rerun V3's measurement on the split page:
- Every function that ran before must still run.
- Per-file coverage is reported, with the goal of 100% for each file.
- Any gap is reported with the specific lines, not summarized.

The results are appended to the baseline analysis doc, so before and after sit side by side.

**V9. `core/` unit tests** run with Node's built-in runner (`node:test`, no new dependency).
They call only the modules' exported functions. Coverage comes from
`node --test --experimental-test-coverage`.

## 6. Order of work

One commit per step, and each step passes V5 and V6 before it is committed:

- **V1–V4.** Test harness over HTTP, the error check, the baseline coverage, and the
  characterization tests, all against the unchanged page. Several commits.
- **S1.** Change `<script>` to `<script type="module" src="frontend/main.js">`, with
  `main.js` holding the whole script unchanged. This proves the loading path before anything
  moves.
- **S2.** Extract `core/` and add its unit tests (V9). Introduce `state` (3a), including the two
  "where you are" values (§4, loop 2).
- **S3.** Extract `infra/`, with the 3b signature changes.
- **S4.** Extract the `ui/` leaf modules: confirm modal, status indicators, markup, page
  chrome, location with `whileApplyingHash`, and charts.
- **S5.** Extract the views: review with `setFlagEditHandlers`, then calendar, conversations
  and analytics.
- **S6.** Extract `app/`, including `refresh-views.js` and `router.js`. `main.js` shrinks to
  wiring.
- **V7–V8.** Add the structural checks, rerun coverage, and write up the before-and-after
  numbers.

## 7. What this verification will not show

- **"Classify with AI" succeeding.** It cannot succeed over HTTP. Only its confirm-and-cancel
  path is tested.
- **Anything against the deployed AWS backend.** The end-to-end tests use the local dev
  backend and its dev login. Nothing in [infra/template.yaml](../../infra/template.yaml) serves this
  page, so there is no deployed copy to test.
- **Browsers other than Chrome.** The Playwright config pins `/usr/bin/google-chrome`.

## Self-critique log

### C1 [RESOLVED]: The loading approach decides whether the end-to-end tests change
Original concern: modules do not load from `file://`, which is how both specs open the page.
**Resolution:** you answered that the page is served only over HTTP. The specs move to HTTP in
[V1 (line 288)](2026-09-30-split-timeline-script.md#L288), and loading is plain modules with no build
step, per [Serving (line 16)](2026-09-30-split-timeline-script.md#L16).

### C2 [RESOLVED]: Splitting may break single-file artifact hosting
**Resolution:** you answered that single-file hosting is no longer needed. Every statement of it
as a goal was removed. What remains is the fact that "Classify with AI" cannot work over HTTP,
recorded under [Out of scope (line 20)](2026-09-30-split-timeline-script.md#L20).

### C3 [RESOLVED]: A naive split creates circular imports
Original concern: I read that the review table calls `setRowOverrides`, which calls
`renderReviewTable`; and that views call `rememberLocation` while `applyLocationHash` calls the
views.
Revision 1's resolution was page events plus splitting the URL writer from the reader. You
pointed out that it did not explain how the loops are resolved. Writing that explanation
showed revision 1 was also incomplete: the URL writer reads `currentConv` and
`currentAnalysis`, which live in the view files.
**Resolution (revision 2):** handlers passed in by `main.js` instead of events; the "where
you are" values moved to `state`; the reader and writer split, with the guard behind
`whileApplyingHash`. All three loops are explained with call sites in
[§4 (line 136)](2026-09-30-split-timeline-script.md#L136). Circular-import detection in
[V7 (line 349)](2026-09-30-split-timeline-script.md#L349) keeps the result checked.

### C4 [RESOLVED]: Imported variables cannot be reassigned
Original concern: `applyExportText` reassigns seven shared globals, which modules forbid.
**Resolution:** a single `state` object; [3a (line 119)](2026-09-30-split-timeline-script.md#L119).

### C5 [RESOLVED]: Infra functions reached into the page
Original concern: `ensureAuthToken` reads a text box and `patchFlagsToBackend` writes the save
indicator, which would make `infra/` depend on the DOM.
**Resolution:** a parameter and a return value respectively;
[3b (line 125)](2026-09-30-split-timeline-script.md#L125).

### C6 [RESOLVED]: The first draft had about 29 files, several under 15 lines
**Resolution:** save status merged into `ui/status-indicators.js`; `renderSubtitle` joined
`switchTab` in `ui/page-chrome.js`; `pearsonR` joined `core/analyses.js`. Revision 2 adds
`app/refresh-views.js`, for 27 files.

### C7 [RESOLVED, gated]: 100% coverage of the browser-side layers is not yet demonstrated
Original concern: the project rule requires it, and nobody has measured what the end-to-end
suite covers.
**Resolution:** coverage is measured before the split, and gaps are closed with
characterization tests before any code moves:
[V3–V4 (line 305)](2026-09-30-split-timeline-script.md#L305). It is measured again after
([V8 (line 359)](2026-09-30-split-timeline-script.md#L359)). **Gate:** V3's numbers; any function
unreachable over HTTP gets reported, not tested around.

### C8 [OPEN]: Line ranges were located by pattern matching, not by reading every function
I built the inventory from function declarations, section banners and keyword counts. The
call sites in §4 were confirmed by reading them. Top-level statements between functions may
belong to a different file than listed. **Open:** each extraction step reads the full range it
moves. V6's moved-versus-changed diff would expose any misplaced statement. Misplacements are
reported in that step's commit message.

### C9 [RESOLVED]: No explanation of how the result is verified
Original concern: you pointed out that revision 1 did not explain how we would know the
program still works.
**Resolution:** [§5 (line 270)](2026-09-30-split-timeline-script.md#L270) sets out
tests-before-moving, moved-versus-changed diffs, structural checks and before-and-after
coverage. [§7 (line 391)](2026-09-30-split-timeline-script.md#L391) states what it will not show.

### C10 [OPEN]: The characterization test list is based on a keyword search
I searched the spec files for element ids such as `toggleShowAuto` and `classifyAiBtn` and
found no matches. I did not read every test body, so a test might exercise these paths by
other selectors. **Mitigation in plan:** V3's coverage run decides what is actually untested.
The V4 list is a starting point. **Open:** revised after V3 runs.
