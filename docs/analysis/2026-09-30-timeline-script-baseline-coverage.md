# Coverage of timeline.html's script, before and after the module split

Measures which lines of the page's JavaScript the end-to-end tests actually run, as
steps V3, V4 and V8 of [the split plan](../plans/2026-09-30-split-timeline-script.md).

**How it was measured.** Chrome's precise coverage, recorded per test by
[e2e/coverage.js](../../e2e/coverage.js) when `COVERAGE_DIR` is set and merged by
[e2e/coverage-report.js](../../e2e/coverage-report.js). A line counts as run if the code at its
first non-blank character ran. Blank and comment-only lines are not counted.

Command, from `e2e/` with Node 20 on the path:

```
COVERAGE_DIR=<dir> npx playwright test
node coverage-report.js <dir> ..
```

## Before the split: the single-file page

The page measured is `timeline.html` at commit 012fec5, after "Classify with AI" was deleted
(step D1) and before any script moved. It has 1,482 counted code lines.

| Measurement | Tests | Result | Lines run |
|---|---|---|---|
| V3: existing suite | 21 | all passed | 1,209 of 1,482 (81.6%) |
| V4: existing suite plus 25 characterization tests | 46 | all passed | 1,456 of 1,482 (98.2%) |

A 26th test, for a plain-text error body (line 982), was added after that run and passed on its own. It is committed with the others in 46a649f. Its effect on coverage is measured in V8, not here.

### Measurement problems found and fixed along the way

- **Reloads lost coverage.** The first V3 run reported `approveRow`, `setRowOverrides` and
  `patchFlagsToBackend` as never called. The Approve test does call them, but then reloads, and
  Chrome discards a document's coverage when the page navigates away. The recorder now saves
  coverage before every `page.goto` and `page.reload`. With that fix, the same 21 tests measure
  81.6%, up from 78.7%.
- **One file counted twice.** The `?api_base=` test loads the page under a different URL, which
  the report first treated as a second file. It now merges by path.

### The 26 lines no test runs, and why

| Lines at 012fec5 | Code | Why no end-to-end test reaches it |
|---|---|---|
| 847–848, 852 | `unwrapUploadedJSON`: the bare-array export and the "neither shape" error | The page only ever parses `GET /export`'s output, which is always `{"conversations": [...]}`. This function moves to `core/export-format.js`, where the unit tests (V9) call it directly. |
| 982 | `describeFailure`: an error body that is plain text, not JSON | Covered by the 26th test, added after this run. |
| 984–985 | `describeFailure`: an error response whose body can't be read | Not attempted: I know of no way to make a substituted response's body fail to read, but haven't verified that none exists. |
| 1031–1037 | `formatEta` | Only called with a time estimate, which needs a transfer lasting over a second (next row). It moves to `core/format.js`, where the unit tests call it directly. |
| 1054 | `makeRateEstimator` returning an estimate | Needs a transfer that runs for more than a second. The tests' uploads and downloads finish sooner. |
| 1295–1297 | Download progress when the response has no `Content-Length` | The local backend's export downloads carry one. I believe a response substituted by a test would also get one, but I didn't try it. |
| 1474–1476 | Saving a flag with no login token | The token is cleared only by "Load a different file" and by a failed restore. Both leave the load screen showing, so no flag can be edited. |
| 1480–1482 | Saving a flag whose message has no server-side id | Every export from the backend carries the ids. |
| 1535–1537 | Exporting with nothing loaded | The export button is only visible once data is loaded. |
| 1073 (`xhr.onabort`) | An upload aborted by the page | Nothing in the page aborts the upload. The handler is unreachable code. |

### What the characterization tests showed about the page

None of the 26 new tests found the page misbehaving. Five failed on their first run against the
unchanged page, all from wrong assumptions in the tests:

- A calendar session can contain only Claude's messages, for example a single reply at 5:33 AM
  in the fixture. Clicking it opens the review tab with 0 messages, because the review tab lists
  only your messages. The tests now pick the busiest session. Whether an empty review is the
  right response to that click is a design question this work doesn't change.
- A trend or scatter chart shows "Not enough data yet." only at zero points, not at one.
- The upload's PUT goes to `/_dev/local-storage/put/…`, not `/uploads/…`.
- A conversation list item's text starts with a blank line, so a test reading its first line got
  an empty name.

## After the split

Measured at commit edc3a10, with the script split into 23 modules under
[frontend/](../../frontend/). Counted code lines rise from 1,482 to 1,575 because `import` and
`export` lines and the new helpers (`whileApplyingHash`, `setFlagEditHandlers`,
`showFirstReviewPage`, `refreshAllViews`, `clearAuthToken`) are code too.

| Suite | Tests | Result | Lines run |
|---|---|---|---|
| End-to-end | 47 | all passed | 1,553 of 1,575 (98.6%) |
| Unit tests for `core/`, plus structural checks | 34 | all passed | 100% of every `core/` file's lines |

**Every function that ran before the split still runs.** The only named functions no end-to-end
test calls are the same two as before: `formatEta` and `xhr.onabort`.

Per file, end-to-end only (18 of 23 files at 100%):

| File | Lines run | Lines not run | Same case as before the split |
|---|---|---|---|
| core/export-format.js | 56 of 59 | 28–29, 33 | Bare-array export and the "neither shape" error (lines 847–848, 852). Unit tests run them. |
| core/format.js | 25 of 31 | 14–19 | `formatEta` (1031–1037). Unit tests run it. |
| infra/api-client.js | 84 of 90 | 78–79, 131–132, 136–137, `xhr.onabort` | Unreadable error body (984–985), no login token (1474–1476), no server-side id (1480–1482), aborted upload (1073). |
| ui/annotated-export.js | 37 of 40 | 17–19 | Exporting with nothing loaded (1535–1537). |
| ui/load-flow.js | 158 of 161 | 209–211 | Download without `Content-Length` (1295–1297). |
| ui/widgets/status-indicators.js | 51 of 52 | 62 | Time-remaining estimate (1054). |

Line 982, a plain-text error body, was unreached in V4's run. It now runs, through the test
added after that run.

**Counting both suites together**, only the `infra/` and `ui/` rows above stay unreached: 13
lines plus `xhr.onabort`. None of them can be reached through the page, for the reasons in the
table of the 26 lines before the split.

### Branches the core unit tests don't take

Line coverage of `core/` is 100%. Branch coverage is 96.2% for `core/analyses.js` and 97.6% for
`core/flags.js`. Each of the three untaken branches guards a case real data can't produce:

| Where | Branch | Why it never happens |
|---|---|---|
| core/analyses.js:49 | `b.count ? … : 0` in friction by session | `buildBlocks` never makes a session with no messages. |
| core/analyses.js:86 | `total ? … : 0` in the trend | A bucket exists only once a message has been counted into it. |
| core/flags.js:52 | `if(!best) return;` in `attachFlags` | The line above returns when the conversation has no sessions. Otherwise the first session is always closer than infinity, so `best` is always set. |

They are unreachable code. Removing them is a behavior-free change outside this plan, so they
stay for now.
