# Split timeline.html's script into modules

**Status:** revision 3, awaiting review.
- Revision 2 recorded that the page is served only over HTTP and that nothing needs to preserve
  single-file hosting.
- Revision 3 adopts your folder layout: `ui/` holds the actions that span views, above
  `ui/views/`, which sits above `ui/render/`, `ui/widgets/` and `ui/navigation/`. It also states
  the purpose of every file.

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
- **View**: code that draws one area of the page from the loaded data. Examples are the
  calendar, the conversation list and the review table.

```
frontend/
  main.js                 startup and event wiring
  ui/*.js                 actions that span several views
  ui/views/               one file per area of the page
  ui/render/              turns data into HTML or SVG for a view to insert
  ui/widgets/             self-contained controls the whole page uses
  ui/navigation/          keeps track of which view is showing
  infra/                  network and browser storage (no DOM)
  core/                   pure data logic (no DOM, no network, no storage)
```

Who may import whom:
- `main.js` may import anything.
- Files directly in `ui/` may import `ui/views/` and everything below it. They may import each
  other only as listed in the import table in §4. Today that means `ui/refresh-views.js` and
  `ui/router.js`, which import no other file at their own level.
- `ui/views/` may import `ui/render/`, `ui/widgets/`, `ui/navigation/`, `infra/` and `core/`.
  One view may import another only as listed in the import table in §4.
- `ui/render/`, `ui/widgets/` and `ui/navigation/` may import only `core/`, and only within
  their own folder.
- `infra/` may import only `core/`.
- `core/` may import nothing outside `core/`.

The `core/` rule mirrors the backend rule that domain code never imports infrastructure. It also
means every `core/` module can be unit-tested in Node without a browser.

New directory: `frontend/` at the repo root. [timeline.html](../../timeline.html) stays where it is so
the [README.md](../../README.md), [.vscode/tasks.json](../../.vscode/tasks.json) and
[scripts/test-dev-up.sh](../../scripts/test-dev-up.sh) references keep working.

## 2. Inventory

Each file gets a purpose statement and its contents. Line numbers are current positions in
[timeline.html](../../timeline.html), and sizes are approximate. I wrote the purposes from the
comments above each function and from the call sites I read; C8 records where that falls short
of reading every body.

### core/ — pure data logic

**`core/export-format.js`** (~90 lines, from [939–1025](../../timeline.html#L939))
- **Purpose:** turns the export text the backend sends into the page's in-memory data. That
  data is a list of conversations, every message, just the messages you wrote, and any flags
  already embedded in the file.
- **Contents:** `FORMAT_VERSION`, `extractMessageText`, `unwrapUploadedJSON`,
  `parseUploadedConversations`.

**`core/state.js`** (~55 lines, from [1449–1468](../../timeline.html#L1449) and [1495](../../timeline.html#L1495))
- **Purpose:** the single holder of everything the page currently knows. Every other module
  reads and writes the shared data here rather than keeping its own copy.
- **Contents:**
  - The loaded data: conversations, the raw export (kept for re-export), all messages, your
    messages, a lookup of your messages by id, and the session blocks.
  - Your flag corrections.
  - The three show-automatic, show-mine and show-replies switches.
  - `GAP_THRESHOLD_SEC`.
  - The two "where you are" values, which today sit beside their views: the open conversation
    (`currentConv`, [line 2109](../../timeline.html#L2109)) and the shown analysis (`currentAnalysis`,
    [line 2709](../../timeline.html#L2709)). §4, loop 2 explains why they move here.

**`core/flags.js`** (~70 lines, from [1470–1503](../../timeline.html#L1470), [1553–1578](../../timeline.html#L1553) and [2659](../../timeline.html#L2659))
- **Purpose:** decides whether a message counts as flagged. The answer combines the
  backend's automatic flags, your corrections and the show switches. The same answer then
  gets attached to each session block, so every view shows the same counts.
- **Contents:** `hasUserValue`, `effectiveFlag`, `isOverridden`, `isFlagged`, `attachFlags`.

**`core/blocks.js`** (~50 lines, from [1504–1552](../../timeline.html#L1504))
- **Purpose:** groups messages into sessions. A session is a run of messages in one
  conversation on one local calendar day, split wherever 15 minutes or more pass with no
  activity. The calendar draws these as bars, and several analyses count them.
- **Contents:** `localDateKey`, `buildBlocks`.

**`core/format.js`** (~45 lines, from [1134–1154](../../timeline.html#L1134) and [1955–1975](../../timeline.html#L1955))
- **Purpose:** turns numbers and timestamps into short readable text: file sizes, time
  remaining, durations, clock times, and day and month headings.
- **Contents:** `formatBytes`, `formatEta`, `fmtDuration`, `fmtClock`, `fmtDayHeading`,
  `fmtMonthHeading`.

**`core/analyses.js`** (~170 lines, from the compute half of each section in [2751–2994](../../timeline.html#L2751) and [2663–2676](../../timeline.html#L2663))
- **Purpose:** computes the numbers behind the five analyses on the Analytics tab. They are:
  - a ranking of conversations or sessions by share of flagged messages
  - flag rate by week or month
  - session length against flag rate
  - flag rate by hour and weekday
  - idle time before a session against flag rate

  The file computes only; it draws nothing.
- **Contents:** `pearsonR` and the five `compute…Analysis` functions. Separating them from
  drawing takes the edit in §3d.

**`core/classify-prompt.js`** (~55 lines, from [1690–1740](../../timeline.html#L1690))
- **Purpose:** builds the prompt text for "Classify with AI" from a batch of your messages.
  Each message goes in with the reply before it, escaped so message text cannot break the
  prompt's structure.
- **Contents:** `CLASSIFY_BATCH_SIZE`, `CLASSIFY_MODEL`, `getPriorReplyText`,
  `escapeForPromptTags`, `buildClassifyPrompt`.

### infra/ — network and storage, no DOM

**`infra/api-client.js`** (~140 lines, from [1045–1099](../../timeline.html#L1045), [1174–1216](../../timeline.html#L1174) and [1624–1647](../../timeline.html#L1624))
- **Purpose:** everything that talks to the timeline backend. It works out the backend's
  address, gets a development login token, uploads with progress and downloads with progress.
  It saves your flag corrections and turns failed responses into readable messages.
- **Contents:** `resolveApiBase`, `API_BASE`, the auth token, `ensureAuthToken`,
  `describeFailure`, `putWithProgress`, `readBodyWithProgress`, `patchFlagsToBackend`.

**`infra/anthropic-classifier.js`** (~70 lines, from [1741–1808](../../timeline.html#L1741))
- **Purpose:** sends one batch of messages to Anthropic's API and reads back which ones it
  judged critical or angry, retrying once on failure. Over HTTP this call cannot succeed (see
  Out of scope).
- **Contents:** `classifyBatchWithAI`, `classifyBatchWithRetry`.

**`infra/artifact-storage.js`** (~40 lines, from [1580–1623](../../timeline.html#L1580))
- **Purpose:** saves "Classify with AI" results as they arrive, using `window.storage`. That
  storage exists only when the page runs inside Claude as an artifact. Over HTTP it is absent,
  and this file does nothing.
- **Contents:** `AUTO_CACHE_KEY`, `hasStorage`, `saveAutoClassificationsToStorage`.

### ui/render/ — turns data into HTML or SVG

**`ui/render/markup.js`** (~90 lines, from [2196–2281](../../timeline.html#L2196))
- **Purpose:** makes text safe to put into the page, and renders message text written in
  Markdown (headings, bold, lists, code) as HTML. It escapes the text first so a message
  cannot inject its own HTML.
- **Contents:** `escapeHtml`, `renderMarkdownLite`.

**`ui/render/charts.js`** (~110 lines, from [2995–3105](../../timeline.html#L2995))
- **Purpose:** draws the bar, line and scatter charts the Analytics tab uses, as SVG inside a
  given element.
- **Contents:** `renderBarChartSVG`, `renderLineChartSVG`, `renderScatterChartSVG`.

### ui/widgets/ — self-contained controls the whole page uses

**`ui/widgets/confirm-modal.js`** (~25 lines, from [912–938](../../timeline.html#L912))
- **Purpose:** the page's own yes/no dialog. It is a replacement for the browser's
  `confirm()`, which some embedding contexts block silently.
- **Contents:** `showConfirm`.

**`ui/widgets/status-indicators.js`** (~100 lines, from [1026–1044](../../timeline.html#L1026), [1100–1133](../../timeline.html#L1100), [1155–1173](../../timeline.html#L1155), [1290–1302](../../timeline.html#L1290) and [1648–1657](../../timeline.html#L1648))
- **Purpose:** the page's progress and status lines. It covers:
  - the load screen's message and progress bar, with its time-remaining estimate
  - the notice that a previous session was restored
  - the "Saved." / "Could not save" line
- **Contents:** `setLoadStatus`, `showLoadProgress`, `hideLoadProgress`, `failLoadProgress`,
  `setLoadProgressIndeterminate`, `makeRateEstimator`, `showRestoredNotice`, `setSaveStatus`.

### ui/navigation/ — keeps track of which view is showing

**`ui/navigation/tabs.js`** (~10 lines, from [2283–2290](../../timeline.html#L2283))
- **Purpose:** switches between the Calendar, Conversations, "Review & flags" and Analytics
  tabs. It highlights the chosen tab button, shows that tab's view and hides the others.
- **Contents:** `switchTab`.

**`ui/navigation/location.js`** (~45 lines, from [2292–2326](../../timeline.html#L2292))
- **Purpose:** records where you are in the web address, such as `#conversations/3`, whenever
  you change tabs, open a conversation or pick an analysis. The browser's Back button then
  returns to the previous view instead of leaving the page, and a reload reopens the same view.
  The reverse direction, reading the address, is in `ui/router.js` (§4, loop 3).
- **Contents:** `currentLocationHash`, `rememberLocation`, and `whileApplyingHash`, which
  replaces the `APPLYING_HASH` flag (§4, loop 3).

### ui/views/ — one file per area of the page

**`ui/views/header.js`** (~10 lines, from [1976–1983](../../timeline.html#L1976))
- **Purpose:** fills in the line under the page title, for example "412 conversations, 18,300
  messages, 2025-01-04 to 2026-09-20."
- **Contents:** `renderSubtitle`.

**`ui/views/calendar.js`** (~100 lines, from [1952–1953](../../timeline.html#L1952) and [1984–2079](../../timeline.html#L1984))
- **Purpose:** draws the Calendar tab. It shows one row per day, with a colored bar for each
  session and markers for flagged messages. Clicking a day, a session or a marker opens the
  matching messages in the review tab.
- **Contents:** `PALETTE`, `colorFor`, `renderCalendar`.

**`ui/views/conversations.js`** (~115 lines, from [2080–2195](../../timeline.html#L2080))
- **Purpose:** draws the Conversations tab. It has a searchable list of conversations and the
  transcript of the one you open, with links from its flags into the review tab.
- **Contents:** `renderConvList`, `selectConversation`.

**`ui/views/review.js`** (~285 lines, from [2356–2629](../../timeline.html#L2356))
- **Purpose:** draws the "Review & flags" tab, where you check and correct flags. It has:
  - a paged, searchable, filterable table of your messages with a checkbox per flag
  - the banner that explains the current filter, with previous-day, next-day and clear
    buttons
  - the entry points other views use to open it on a given conversation, time span or day
- **Contents:**
  - `PAGE_SIZE` and the review filter state
  - `getFilteredHumanMessages`, `jumpToReview`, `jumpToReviewDay`, `shiftReviewDay`,
    `clearReviewFilters`
  - `checkboxCell`, `renderReviewFilterBanner`, `renderReplyRow`, `renderReviewTable`
  - `setFlagEditHandlers` (§4, loop 1)

**`ui/views/analytics.js`** (~180 lines, from [2680–2749](../../timeline.html#L2680) and the render half of each section in [2786–2994](../../timeline.html#L2786))
- **Purpose:** draws the Analytics tab. It runs the chosen analysis from `core/analyses.js` in
  small chunks so the progress bar moves and the page stays responsive. It then draws the
  result with its chart, and results link through to the review tab.
- **Contents:**
  - `ANALYTICS_META`, `computeWithProgress`, `showAnalyticsProgress`, `setAnalyticsProgress`,
    `runAnalysis`
  - the five `render…Result` functions

### ui/ — actions that span several views

**`ui/load-flow.js`** (~210 lines, from [1217–1289](../../timeline.html#L1217) and [1303–1434](../../timeline.html#L1303))
- **Purpose:** getting data onto the screen. The steps are:
  1. Upload the file you pick.
  2. Optionally run the backend's flag detection, page by page with progress.
  3. Download the processed export and hand it to `core/export-format.js`.
  4. Draw every view.

  On page load, it also reopens your last session if the backend still has it, then opens the
  view named in the web address.
- **Contents:** `applyExportText`, `tryRestoreSession`, `runDetectionPass`, `handleLoadClick`.

**`ui/refresh-views.js`** (~15 lines, from [1667–1672](../../timeline.html#L1667), [1889–1893](../../timeline.html#L1889) and [2638–2642](../../timeline.html#L2638))
- **Purpose:** after any change to flags, recomputes them and redraws the calendar,
  conversation list, open conversation and review table. Today that sequence is copied in
  three places.
- **Contents:** `refreshAllViews`.

**`ui/flag-edits.js`** (~45 lines, from [1658–1684](../../timeline.html#L1658) and [2635–2643](../../timeline.html#L2635))
- **Purpose:** handles your flag changes. Ticking a checkbox or pressing Approve records all
  three flags on that message as yours and saves them to the backend. Flipping the
  show-automatic or show-mine switches changes what every view counts. Either way, every view
  gets redrawn.
- **Contents:** `setRowOverrides`, `approveRow`, `onVisibilityToggleChanged`.

**`ui/classify-run.js`** (~95 lines, from [1809–1901](../../timeline.html#L1809))
- **Purpose:** the "Classify with AI" button. It asks for confirmation, sends your messages
  in batches through `infra/anthropic-classifier.js`, records the results as automatic flags
  and redraws.
- **Contents:** `classifyWithAI`.

**`ui/annotated-export.js`** (~50 lines, from [1902–1951](../../timeline.html#L1902))
- **Purpose:** the download button. It saves the conversations with both the automatic flags
  and your corrections written into each message, in a file this page can load again.
- **Contents:** `exportAnnotatedConversations`.

**`ui/router.js`** (~30 lines, from [2327–2348](../../timeline.html#L2327))
- **Purpose:** the reverse of `ui/navigation/location.js`. When the page loads or you press
  Back, it reads the web address and opens the tab, conversation or analysis it names.
- **Contents:** `applyLocationHash`.

### Top level

**`main.js`** (~60 lines, from [1435–1447](../../timeline.html#L1435), [2349–2354](../../timeline.html#L2349), [2630–2653](../../timeline.html#L2630), [2746–2749](../../timeline.html#L2746) and [3106](../../timeline.html#L3106))
- **Purpose:** the page's starting point. It connects every button, box and switch in the
  markup to the code that handles it, and hands the flag-edit handlers to the review table
  (§4, loop 1). Then it tries to restore your last session.
- **Contents:** every top-level `addEventListener` call, the `setFlagEditHandlers` call, and
  the startup call to `tryRestoreSession`.

That makes 28 files. The largest is `ui/views/review.js` at about 285 lines, under the 300–400
soft target.

## 3. Edits the split forces

**3a. Shared state becomes one object.** A module cannot reassign a variable it imported. But
[applyExportText (line 1221)](../../timeline.html#L1221) reassigns `CONVERSATIONS`, `MESSAGES`,
`BLOCKS` and four others. So `core/state.js` exports one `state` object, and every reference
changes from `CONVERSATIONS` to `state.conversations`. This is the largest mechanical edit and
touches every module that reads the data.

**3b. Infra functions stop touching the page.**
- `ensureAuthToken` reads the login box directly ([line 1063](../../timeline.html#L1063)). It
  will take the login name as a parameter, and `ui/load-flow.js` reads the box.
- `patchFlagsToBackend` calls `setSaveStatus` four times
  ([lines 1626–1644](../../timeline.html#L1626)). It will return the message instead, and
  `ui/flag-edits.js` shows it.

**3c. Cross-file names are imported and exported.** Every function another file calls gets
`export` in front of it, and the calling file gets an `import` line.

**3d. Analyses compute without drawing.** Today each `compute…Analysis` function does three
things:
- It runs its loop through `computeWithProgress`, which uses the browser-only
  `requestAnimationFrame` ([line 2692](../../timeline.html#L2692)).
- It calls its own `render…Result` as its last line, for example
  [line 2783](../../timeline.html#L2783).
- `runAnalysis` starts it and never renders anything itself
  ([lines 2726–2745](../../timeline.html#L2726)).

As written, the compute functions cannot live in `core/`. Two small edits per function fix
that:
- The compute function receives the chunked loop as a parameter, `runChunked(items, fn)`,
  instead of calling `computeWithProgress` directly. `ui/views/analytics.js` passes one that
  wraps `computeWithProgress` and updates the progress bar. Unit tests pass a plain
  synchronous loop.
- The compute function returns its result instead of calling the renderer. Each renderer's
  parameters become one object, for example `renderFrictionResult({rows, granularity})`, and
  `runAnalysis` passes the returned object to the matching renderer.

The page draws the same thing at the same point, after the computation finishes.

**3e. The unused `analysisCache` is dropped.** It is declared at
[line 2710](../../timeline.html#L2710), and nothing in the file reads or writes it.

## 4. How the loops between files are resolved

A **circular dependency** is two or more files that import each other, directly or through a
chain. The project rules forbid them. If today's functions were simply cut into the files
above, there would be three loops. Each one is listed below with the call sites I read that
create it, and how it is removed.

A runtime call loop is different from an import loop. Clicking a checkbox in the review table
makes the review table redraw itself, and that stays true after the split. What goes away is
two *files* each naming the other in their imports. Nothing changes about what happens when you
click.

### Loop 1: review table → flag edits → review table (and the other views)

Today:
- The review table's checkboxes and Approve buttons call `setRowOverrides` and `approveRow`
  ([lines 2595–2603](../../timeline.html#L2595)).
- `setRowOverrides` redraws the calendar, the conversation list, the open conversation and the
  review table ([lines 1667–1672](../../timeline.html#L1667)).
- `selectConversation` calls `jumpToReview` ([lines 2165–2192](../../timeline.html#L2165)).

Cut naively, `ui/views/review.js` imports `ui/flag-edits.js`. That file imports
`ui/views/review.js` and `ui/views/conversations.js`, which in turn imports
`ui/views/review.js`. That is a loop. It also breaks the layer rule, because a view would
import a file from the level above it.

Fix: `ui/views/review.js` does not import the flag-edit functions. It exports a setup function
that receives them:

```js
// ui/views/review.js
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
import { setFlagEditHandlers } from './ui/views/review.js';
import { setRowOverrides, approveRow } from './ui/flag-edits.js';
setFlagEditHandlers(setRowOverrides, approveRow);
```

The resulting import chains all point downward:
- `main.js` → `ui/flag-edits.js` → `ui/refresh-views.js` → `ui/views/review.js`
- `main.js` → `ui/views/review.js`

`ui/views/review.js` imports nothing from the level above.

I chose passing functions in over firing named page events (what revision 1 proposed). The
reason is readability, which prompted this plan. A reader who searches for
`setFlagEditHandlers` finds the single line in `main.js` that says exactly which function runs.
An event named by a string can have any number of listeners anywhere, found only by searching
for the string.

The same redraw sequence is copied in `setRowOverrides`, `classifyWithAI`
([lines 1889–1893](../../timeline.html#L1889)) and `onVisibilityToggleChanged`
([lines 2638–2642](../../timeline.html#L2638)). It becomes one function, `refreshAllViews`, in
`ui/refresh-views.js`, and all three call it. `applyExportText`'s redraw
([lines 1244–1247](../../timeline.html#L1244)) is different: it also redraws the subtitle and
does not reopen a conversation. It stays as it is.

### Loop 2: views → address writer → views

Today:
- `selectConversation` and `runAnalysis` call `rememberLocation`
  ([line 2112](../../timeline.html#L2112), [line 2729](../../timeline.html#L2729)) to record where you
  are in the web address.
- `rememberLocation` calls `currentLocationHash`, which reads `currentConv` and
  `currentAnalysis` ([lines 2311–2319](../../timeline.html#L2311)). Those variables are declared
  beside `selectConversation` and `runAnalysis`.

Cut naively, `ui/views/conversations.js` imports `ui/navigation/location.js` for
`rememberLocation`. Meanwhile `ui/navigation/location.js` imports `ui/views/conversations.js`
for `currentConv`. The same is true for analytics.

Fix: the two "where you are" values move into `core/state.js`, as
`state.selectedConversation` and `state.selectedAnalysis`. `selectConversation` and
`runAnalysis` write them there, and `ui/navigation/location.js` reads them from there. Now
`location.js` imports only `core/state.js`. The views import it, and nothing imports back.

### Loop 3: address reader → views → address reader

Today, `applyLocationHash` ([lines 2327–2348](../../timeline.html#L2327)) calls `switchTab`,
`selectConversation` and `runAnalysis`. It also sets the `APPLYING_HASH` guard that
`rememberLocation` checks, so that opening a view from the address does not immediately write
the address back.

If `applyLocationHash` shared a file with `rememberLocation`, that file would import the views,
and the views would import it: a loop. Fix: the reader goes in `ui/router.js`, above the
views, and the writer stays in `ui/navigation/location.js`, below them. The guard stays
private to `location.js`, which exports one function for the router to use:

```js
// ui/navigation/location.js
let applyingHash = false;
export function whileApplyingHash(fn){
  applyingHash = true;
  try { fn(); } finally { applyingHash = false; }
}
```

This is the same try/finally that `applyLocationHash` has today, moved behind a function so no
other file can set the flag and forget to clear it.

### The resulting imports

These are the imports between `ui/` files. Imports of `core/` and `infra/` are left out. Each
row reads "this file imports these":

| File | Imports (beyond `core/` and `infra/`) |
|---|---|
| `main.js` | every file directly in `ui/`, `ui/views/review.js` (to call `setFlagEditHandlers`), `ui/navigation/tabs.js` and `ui/navigation/location.js` (the tab buttons switch tabs and record the address, [timeline.html:2351](../../timeline.html#L2351)), and each view for its listeners |
| `ui/router.js` | `ui/navigation/tabs.js`, `ui/navigation/location.js`, `ui/views/conversations.js`, `ui/views/analytics.js` |
| `ui/load-flow.js` | `ui/router.js` (a restored session reopens the view named in the address, [timeline.html:1280](../../timeline.html#L1280)), `ui/widgets/status-indicators.js`, `ui/views/header.js`, `ui/views/calendar.js`, `ui/views/conversations.js`, `ui/views/review.js` |
| `ui/flag-edits.js` | `ui/refresh-views.js`, `ui/widgets/status-indicators.js` |
| `ui/classify-run.js` | `ui/refresh-views.js`, `ui/widgets/confirm-modal.js` |
| `ui/annotated-export.js` | `ui/widgets/status-indicators.js` |
| `ui/refresh-views.js` | `ui/views/calendar.js`, `ui/views/conversations.js`, `ui/views/review.js` |
| `ui/views/calendar.js` | `ui/views/review.js`, `ui/render/markup.js` |
| `ui/views/conversations.js` | `ui/views/review.js`, `ui/navigation/location.js`, `ui/render/markup.js` |
| `ui/views/analytics.js` | `ui/views/review.js`, `ui/navigation/location.js`, `ui/render/charts.js`, `ui/render/markup.js` |
| `ui/views/review.js` | `ui/navigation/tabs.js`, `ui/render/markup.js` |
| `ui/render/charts.js` | `ui/render/markup.js` |

Reading down the table, no file imports anything that imports it back. Three views import
`ui/views/review.js`, because they open the review tab, and it imports none of them. I derived
this table from the call sites quoted in §4 and the call map, not from running a tool. The
check for circular imports (§5, step V7) runs on every test run and would catch a mistake in
it.

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
- The show-automatic and show-mine switches. Asserts all four views redraw. Also the
  show-replies switch, which asserts replies appear in the review table.
- Clicking a friction-ranking row, a calendar session bar, and the "review this conversation"
  control in a conversation. Asserts each lands in the review tab with the right filter. This
  exercises the imports into `ui/views/review.js`.
- Each analysis's granularity buttons (by conversation or session, weekly or monthly). Asserts
  the redrawn result. This exercises the §3d edit.
- Opening `#analytics/<name>` directly and opening `#conversations/<n>` directly. Asserts the
  right view opens and the address is not rewritten on load. This exercises loops 2 and 3 and
  the guard.
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
- A layer check enforces the "who may import whom" rules in §1. It also fails if a `core/`
  file mentions `document`, `window`, `fetch`, `localStorage` or `requestAnimationFrame`, or
  if an `infra/` file mentions `document`.

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
- **S2.** Extract `core/` and add its unit tests (V9):
  - Introduce `state` (3a), including the two "where you are" values (§4, loop 2).
  - Make the §3d analysis edit and drop `analysisCache` (3e).
- **S3.** Extract `infra/`, with the 3b signature changes.
- **S4.** Extract `ui/render/`, `ui/widgets/` and `ui/navigation/`, including
  `whileApplyingHash`.
- **S5.** Extract `ui/views/`: review with `setFlagEditHandlers` first, then header, calendar,
  conversations and analytics.
- **S6.** Extract the files directly in `ui/`, including `refresh-views.js` and `router.js`.
  `main.js` shrinks to wiring.
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
[V1 (line 498)](2026-09-30-split-timeline-script.md#L498), and loading is plain modules with no build
step, per [Serving (line 20)](2026-09-30-split-timeline-script.md#L20).

### C2 [RESOLVED]: Splitting may break single-file artifact hosting
**Resolution:** you answered that single-file hosting is no longer needed. Every statement of it
as a goal was removed. What remains are facts about code that depends on it:
- "Classify with AI" cannot work over HTTP, recorded under
  [Out of scope (line 24)](2026-09-30-split-timeline-script.md#L24).
- `infra/artifact-storage.js` does nothing over HTTP.

### C3 [RESOLVED]: A naive split creates circular imports
Original concern: I read that the review table calls `setRowOverrides`, which calls
`renderReviewTable`; and that views call `rememberLocation` while `applyLocationHash` calls the
views.
Revision 1's resolution was page events plus splitting the address writer from the reader. You
pointed out that it did not explain how the loops are resolved. Writing that explanation
showed revision 1 was also incomplete: the address writer reads `currentConv` and
`currentAnalysis`, which live in the view files.
**Resolution (revision 2):**
- Handlers are passed in by `main.js` instead of events.
- The "where you are" values move to `state`.
- The reader and writer are split, with the guard behind `whileApplyingHash`.

All three loops are explained with call sites in
[§4 (line 345)](2026-09-30-split-timeline-script.md#L345). Circular-import detection in
[V7 (line 561)](2026-09-30-split-timeline-script.md#L561) keeps the result checked.

### C4 [RESOLVED]: Imported variables cannot be reassigned
Original concern: `applyExportText` reassigns seven shared globals, which modules forbid.
**Resolution:** a single `state` object; [3a (line 305)](2026-09-30-split-timeline-script.md#L305).

### C5 [RESOLVED]: Infra functions reached into the page
Original concern: `ensureAuthToken` reads a text box and `patchFlagsToBackend` writes the save
indicator, which would make `infra/` depend on the DOM.
**Resolution:** a parameter and a return value respectively;
[3b (line 311)](2026-09-30-split-timeline-script.md#L311).

### C6 [RESOLVED]: The first draft had about 29 files, several under 15 lines
**Resolution:** save status merged into `ui/widgets/status-indicators.js`, and `pearsonR`
joined `core/analyses.js`. Revision 3 has 28 files. Two of them are about 10 lines:
`ui/navigation/tabs.js` and `ui/views/header.js`. You chose those groupings for clarity of
purpose over file count; see C11.

### C7 [RESOLVED, gated]: 100% coverage of the browser-side layers is not yet demonstrated
Original concern: the project rule requires it, and nobody has measured what the end-to-end
suite covers.
**Resolution:** coverage is measured before the split, and gaps are closed with
characterization tests before any code moves:
[V3–V4 (line 515)](2026-09-30-split-timeline-script.md#L515). It is measured again after
([V8 (line 571)](2026-09-30-split-timeline-script.md#L571)). **Gate:** V3's numbers; any function
unreachable over HTTP gets reported, not tested around.

### C8 [OPEN]: Line ranges and purposes come from pattern matching and comments, not from reading every function
I built the inventory from function declarations, section banners, keyword counts and the
comment above each function. The call sites in §4 and §3d were confirmed by reading them.
Top-level statements between functions may belong to a different file than listed, and a
purpose statement may miss something a function body does. C13 is an example of what this
approach missed once. **Open:** each extraction step reads the full range it moves. V6's
moved-versus-changed diff would expose any misplaced statement. Misplacements and corrected
purposes are reported in that step's commit message.

### C9 [RESOLVED]: No explanation of how the result is verified
Original concern: you pointed out that revision 1 did not explain how we would know the
program still works.
**Resolution:** [§5 (line 480)](2026-09-30-split-timeline-script.md#L480) sets out
tests-before-moving, moved-versus-changed diffs, structural checks and before-and-after
coverage. [§7 (line 604)](2026-09-30-split-timeline-script.md#L604) states what it will not show.

### C10 [OPEN]: The characterization test list is based on a keyword search
I searched the spec files for element ids such as `toggleShowAuto` and `classifyAiBtn` and
found no matches. I did not read every test body, so a test might exercise these paths by
other selectors. **Mitigation in plan:** V3's coverage run decides what is actually untested.
The V4 list is a starting point. **Open:** revised after V3 runs.

### C11 [RESOLVED]: `app/` was not an application layer, and `ui/` mixed views with other pieces
Original concern: you asked why `ui/` could not call `app/`. The answer was that `app/` held
page-coordinating code that calls the views, so it was really the top of the interface, not an
application layer. Meanwhile `ui/` mixed four views with six non-view pieces.
**Resolution:** your layout is adopted.
- The actions that span views sit directly in `ui/`, above `ui/views/`.
- The non-view pieces went into `ui/render/`, `ui/widgets/` and `ui/navigation/`.
- `page-chrome` was dissolved: `switchTab` went to `ui/navigation/tabs.js`, and `renderSubtitle`
  became the view `ui/views/header.js`.

See [§1 (line 35)](2026-09-30-split-timeline-script.md#L35) and
[§2 (line 79)](2026-09-30-split-timeline-script.md#L79).

### C12 [RESOLVED]: The inventory named functions but not what each file is for
**Resolution:** every file in [§2 (line 79)](2026-09-30-split-timeline-script.md#L79) now opens
with a purpose statement, as you asked.

### C13 [RESOLVED]: Revision 2 placed the analysis computations in `core/`, which they could not join as written
Original concern: found while writing the purpose statements. Each `compute…Analysis`
function calls its own renderer, and runs its loop through `computeWithProgress`, which uses
the browser-only `requestAnimationFrame`. In `core/` they would have imported views and
failed under Node.
**Resolution:** the edit in [3d (line 321)](2026-09-30-split-timeline-script.md#L321).
`computeWithProgress` moves to `ui/views/analytics.js`, and `requestAnimationFrame` is added to
the `core/` layer check in V7. The alternative was keeping computation and drawing together in
`ui/views/analytics.js` with no edit. That would give one ~350-line file with nothing testable
in Node, so I rejected it.

### C14 [RESOLVED]: `analysisCache` is dead code
Original concern: it is declared at [timeline.html:2710](../../timeline.html#L2710) and never used.
Carrying it into a new file would suggest it matters.
**Resolution:** dropped; [3e (line 342)](2026-09-30-split-timeline-script.md#L342). Revert this if
you'd rather the split carry it unchanged.
