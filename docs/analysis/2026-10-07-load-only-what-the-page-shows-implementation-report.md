# "Load only what the page shows": what was done, what broke, what needs you

This report covers the implementation of
[docs/plans/2026-10-06-load-only-what-the-page-shows.md](../plans/2026-10-06-load-only-what-the-page-shows.md)
(called "the plan" below), from commit `53491ed` to `1b60016`. Timing results are in a separate
analysis, [2026-10-06-load-only-what-the-page-shows-measurements.md](2026-10-06-load-only-what-the-page-shows-measurements.md),
and are not repeated here.

Every item has an ID, so you can answer one by its ID alone:

| Prefix | Section | What it holds |
|---|---|---|
| **S** | [Where things stand](#where-things-stand) | Facts about the state of the code and tests today |
| **B** | [Bugs found and fixed](#bugs-found-and-fixed) | Defects Claude found and corrected during the work |
| **Q** | [Decisions for you](#decisions-for-you) | Questions only you can answer; work is waiting on some of them |
| **T** | [Test changes Claude made](#test-changes-claude-made) | Every change to a test that was already committed, beyond what §10b approved, plus how the approved ones were carried out where that involved judgement |
| **O** | [Choices Claude made where the plan was silent](#choices-claude-made-where-the-plan-was-silent) | Design decisions you have not reviewed; each can be reversed |
| **P** | [Problems found](#problems-found) | Things wrong or risky that were not fixed, or that only matter for how the work was done |

## Terms used below

- **Record:** the stored summary of one conversation (name, times, counts, where it came from). One
  per conversation, in the DynamoDB table.
- **Row:** one stored message, or one *note*, in the same table. Flags live on the message's row.
- **Session:** a run of one conversation's messages with no pause of 15 minutes or more. Sessions
  are stored with fourteen counts each, so the page can draw the Calendar without reading messages.
- **Branch / pruning:** a conversation in the export is a tree; editing a message starts a new
  branch. The server keeps the path to the latest message and *prunes* (drops) the branches it
  replaced, leaving a **note** row where each was. A replaced branch long enough to matter is
  instead kept as a conversation of its own.
- **Unknown time:** a message without a `created_at`. It is stored with the date 1970-01-01 as a
  marker meaning "unknown".
- **Time limit (work budget):** each server request does at most about 9 seconds of work, then
  answers with what it has.
- **Part / cursor:** an answer cut short by the time limit is one *part*. It carries a *cursor*,
  an opaque marker of where to carry on, which the page sends back to get the next part.
- **Data version:** a number on the user's record that rises whenever their data changes. A page
  reading several parts checks that it stays the same, and starts over if it changes.
- **Scan:** the server's pass that sets the automatic flags (caps, critical, angry) on your
  messages.
- **Slimming:** the page, before uploading, drops what the server doesn't keep from the export
  (63.5 MB → 9.3 MB for your export) and compresses it (→ 2.8 MB). This runs in a **Web Worker**,
  a background thread of the page, so the page stays responsive.
- **Test levels:** *backend tests* are Rust tests of the server; *page unit tests* run the page's
  JavaScript in Node with stand-ins for the browser; *browser tests* (Playwright, in [e2e/](../../e2e/))
  drive the real page in Chrome against a real local server.
- **Coverage:** the share of code lines that at least one test runs.
- **Pre-existing:** present before this work began (before commit `53491ed`).

---

## Where things stand

### S1. All plan steps are implemented except the AWS half of step 12

Steps 1–11 of the plan's §10 are implemented and committed. Step 12 has two halves:
- **Measurements on this machine:** done.
- **Measurements on AWS:** not done, because they need a deployment that would change the live
  public site. See [Q1](#q1-how-to-deploy-the-dev-stack-which-today-serves-the-public-address).

Nothing is uncommitted. The working tree is clean at `1b60016`.

### S2. Backend tests: 664 of 664 pass

Run with `cargo test --workspace` on 2026-10-06 after the last backend change.

### S3. Page unit tests: 271 of 272 pass

The one failure is the committed test "every kind of event the page listens for is covered by
the activity recorder". The fix needs your approval; see
[Q2](#q2-may-the-activity-inventory-test-allow-the-upload-workers-message-events).

### S4. Browser tests: 105 of 107 pass

Run on 2026-10-06 with the full suite (`npx playwright test` in `e2e/`). The two failures:
- "the open conversation's table of sessions is titled". See
  [Q3](#q3-may-the-sessions-table-title-test-look-only-at-the-sessions-title).
- "the show switches change what every view counts". It fails or passes depending on how fast the
  server answers, and failed in two of the three full runs. See
  [Q4](#q4-may-the-show-switches-test-wait-for-claudes-replies).

### S5. Coverage

- **Page:** 92.78% of lines (93.25% of branches) in the last run.
  - I checked every uncovered line against git's line history (`git blame`). None was written in
    this work; all are older code, such as the Cognito sign-in and parts of the Describe form.
  - Not every *branch* of the new code is covered. For example,
    [`readBackFailed`](../../frontend/ui/page-flow.js#L222)'s "your sign-in ran out" branch is not
    covered, because it can only be reached with the real Cognito sign-in.
- **Backend:** 95.6% of lines, measured with `cargo llvm-cov` before the last backend commit
  (`8e7ba2d`, the part caps of [O20](#o20-a-part-holds-at-most-1000-records-or-2000-sessions)), and
  not measured again since.
  - That commit's new lines are run by its own test,
    `a_part_holds_at_most_its_share_of_records_and_sessions`.
  - The uncovered remainder is code from before this work (the Lambda start-up files, `main.rs`, the
    experiments crate), or error branches the current code cannot reach. Each of those carries a
    comment saying so, as your rule on unreachable backstops asks.

### S6. Where the main pieces live

- **Domain types and rules (no input/output):** [backend/timeline-core/](../../backend/timeline-core/).
  The module list in its [README](../../backend/timeline-core/README.md) is up to date.
- **Storage (in-memory and DynamoDB):** [backend/timeline-storage/](../../backend/timeline-storage/).
- **Server routes and processing:** [backend/timeline-api/](../../backend/timeline-api/).
- **The page:** [frontend/](../../frontend/) and [timeline.html](../../timeline.html).
  - The new upload worker is in [frontend/workers/](../../frontend/workers/).
  - Two vendored libraries were added: [vendor/streamparser-json/](../../vendor/streamparser-json/)
    (MIT) and [vendor/highlight.js/](../../vendor/highlight.js/) (BSD-3-Clause). Each licence was
    confirmed from its own licence file, and each file's checksum is recorded in its README.
- **Measurement script:** [e2e/measure-waits.js](../../e2e/measure-waits.js).

### S7. Who wrote what

- The backend, the browser-test changes, the measurements, and the fixes in
  [B5](#b5-review-no-longer-highlighted-a-sessions-rows)–[B9](#b9-four-browser-tests-passed-without-testing-anything)
  were written by me (the main Claude session).
- The page (steps 8, 10 and 11, page side) was written by a second Claude agent working to my
  instructions. I reviewed its report, re-ran its tests, checked its error handling and coverage,
  and fixed what the browser tests then caught.
  - Its design choices are listed under O, marked "(page agent)", because you have not reviewed
    them.

---

## Bugs found and fixed

### B1. A message from a later file could be mistaken for a revived branch

- **What:** when a later export of a conversation contained a message inside the time range
  already stored, processing treated it as a *revived branch* (a branch pruned earlier that a later
  export continues). That could replace stored messages with it.
- **Fix:** a revival must now actually replace something. Otherwise the plan's time rule (Q18)
  decides what is added. See [`plan_merge`](../../backend/timeline-core/src/merge.rs#L69).
- **Found by:** the new processing tests. Commit `a88ccd0`.

### B2. After two uploads raced, the conversation's record could undercount its rows

- **What:** two files processed at once can both try to update the same conversation. The record
  is written conditionally, so one writer loses and redoes its merge.
- **The bug:** on the redo, it saw the winner's rows as already stored. It then left the record's
  message count lower than the rows actually stored.
- **Fix:** whenever the record and the rows disagree, the record is brought into line with the
  rows. Commit `a88ccd0`.

### B3. A conversation with a message of unknown time lost that message

- **What:** pruning keeps the path to the latest message. A message of unknown time sorted as the
  oldest, so it was pruned away as if it were a replaced branch.
- **Fix:** a conversation with any message of unknown time is now left whole, not pruned; see
  [branches.rs](../../backend/timeline-core/src/branches.rs#L18). This is also choice
  [O14](#o14-a-conversation-with-any-message-of-unknown-time-is-not-pruned). Commit `a88ccd0`.

### B4. The annotated download left out conversations with no messages

- **Fix:** the download's last part now adds them. Commit `a88ccd0`.

### B5. Review no longer highlighted a session's rows

- **What:** clicking a session bar or a flag icon on the Calendar or the Conversations tab used to
  open Review with that session's messages highlighted.
  - The new page lost this because sessions now arrive as counts, without their messages' ids, so
    the page had nothing to highlight.
  - Three committed browser tests failed on it.
- **Fix:** the page now picks the rows to highlight from the rows the server sends: all of your
  messages for a session click, or those with the clicked flag in effect for a flag click. It waits
  until a part actually contains such a row.
  - See [`showHighlights`](../../frontend/ui/views/review.js#L418) and the callers in
    [calendar.js](../../frontend/ui/views/calendar.js#L109) and
    [conversations.js](../../frontend/ui/views/conversations.js#L285).
  - Commit `2359ce0`, with a new page unit test.
- **Related choice:** [Q6](#q6-should-clicking-a-flag-icon-filter-review-to-that-flag).

### B6. A failed read-back after saving a file's details left Describe

- **What:** after you press Done on Describe, the page saves the details and then reads the
  timeline again. If that read failed, the new page showed the error in the loading box and left
  the Describe page.
  - The committed browser test "a save whose details can't be read back stays on Describe and says
    so" expects the old behaviour.
- **Fix:** a failed read-back now closes the loading box, stays on Describe, and shows "Could not
  read your files' details: …" there. See [`readBackFailed`](../../frontend/ui/page-flow.js#L222).
  Commit `c4ba263`.
- **Related test change:** [T1](#t1-a-new-page-unit-test-was-changed-to-expect-the-opposite-of-what-it-first-checked).

### B7. A batch's files could be sent out of order

- **What:** each file of a batch is now prepared in its own worker. Whichever finished preparing
  first was sent first, so a small second file could overtake a large first one.
  - The committed browser test "Stop with one file processed and one still sending" assumes the
    first file is sent first. It failed in 2 of the 4 runs that included it.
- **Fix:** files are still prepared at the same time, but each waits to start sending until the
  file before it has started (or failed, or been stopped). See
  [load-flow.js](../../frontend/ui/load-flow.js#L285). Commit `a1635fe`.
  - The new page unit test for this fails without the fix (checked).

### B8. Two error handlers in Review dropped errors silently

- **What:** when a newer Review request replaced an older one and the older one then failed, its
  error was dropped with no log. That breaks your no-silent-swallow rule.
- **Fix:** the error is now logged as information ("a replaced page request stopped: …"); see
  [review.js:207](../../frontend/ui/views/review.js#L207) and
  [review.js:162](../../frontend/ui/views/review.js#L162). No message is shown on screen, since that
  request no longer matters. Commit `2359ce0`.

### B9. Four browser tests passed without testing anything

These tests still intercepted the old timeline download (`GET /export` on page load), which the
page no longer makes, so their setup did nothing and they passed regardless. §10b approved
changing each one.
- **"…signed in in the same tab…"**
  ([cognito-login.spec.js](../../e2e/cognito-login.spec.js)) and the `slowExport` helper in
  [views.spec.js](../../e2e/views.spec.js): they held back the download so the loading box could be
  seen. They now hold back `GET /sessions`.
- **"a session that cannot be fetched on reload…"** ([views.spec.js](../../e2e/views.spec.js)): it
  now fails `GET /sessions`.
- **"a file bigger than axum's default 2MB body limit still uploads"**
  ([upload-flow.spec.js](../../e2e/upload-flow.spec.js)): slimming shrank the file well under 2 MB,
  so the server's size limit was never reached.
  - It now opens the page with `&upload=unslimmed`, so the file is sent as it is.
  - It checks that the bytes handed to the upload request equal the file's 2,646,984 bytes.

Commit `22b1d15`.

### B10. The API template never routed PUT, so editing details could not work on AWS

- **What:** [infra/template.yaml](../../infra/template.yaml#L396) had no PUT route to the API, and its
  CORS rules (which browser origins may call which methods) did not allow PUT.
  - Editing a file's or conversation's details uses PUT, so it could never have worked on the
    deployed stack.
- **Fix:** route and CORS added (commit `e34cedd`). See also
  [T5](#t5-the-template-test-now-expects-put-among-the-allowed-methods).

### B11. The processing measurement example was already broken

- **What:** [examples/measure_processing.rs](../../backend/timeline-api/examples/measure_processing.rs)
  never recorded the upload's facts before processing, so it failed.
  - This was broken before this work began.
- **Fix:** it now records them.

### B12. The core README described the old ports

- **What:** the core README's module table and diagram predate this work.
- **Fix:** the new modules and port methods were added (commit `6784c3e`).

---

## Decisions for you

### Q1. How to deploy the dev stack, which today serves the public address

**Facts (observed 2026-10-06):**
- `timeline-dev` is the only application stack in the account.
- Its CloudFront distribution `E6MWRB0WWMX8K` (CloudFront is Amazon's content-delivery network,
  which serves the page) answers for **howangryami.telotrope.ai**, with the public certificate.
  That name resolves to this distribution.
- The committed [infra/samconfig.toml](../../infra/samconfig.toml) gives dev the name
  **dev.howangryami.telotrope.ai** and a different certificate. It was committed after the last dev
  deploy (2026-10-02 22:01 UTC).
- I made a trial deploy, answering "no" at its prompt; its change set was deleted and nothing was
  applied. The planned changes it listed:
  - removing the MessageFlags table, which I counted at 0 items first, so nothing would be lost;
  - adding the PUT route;
  - updating the functions;
  - changing the sign-in client and the distribution to the new name.

**So a deploy today must do one of:**
- **(a) Deploy as committed.** The site moves to dev.howangryami.telotrope.ai, and
  howangryami.telotrope.ai stops working until something else serves it.
- **(b) Keep today's name** (`scripts/deploy.sh dev PageDomain=howangryami.telotrope.ai
  PageCertificateArn=<public certificate>`). The new API replaces the one under the live page.
  The deploy script also publishes the new page in its last step, so the page and the API would
  match, but the public address would then show this new, partly tested version.
- **(c) Something else**, such as creating a separate public stack first.

**Also needed for the AWS measurements:** the page signs in on AWS through Cognito (Amazon's
sign-in service), and the measurement script only knows local development's sign-in. Either:
- **(i)** I create a temporary user in the dev pool and delete it after the run, along with that
  user's stored data; or
- **(ii)** you sign in and run the upload yourself while I read the logs.

**Waiting on this:** step 12's AWS measurements, at 512 MB and at 1,769 MB of processing memory.

### Q2. May the activity-inventory test allow the upload worker's message events?

- **The test:** [activity-inventory.test.js:67](../../frontend/tests/activity-inventory.test.js#L67)
  lists every kind of event the page listens for. It fails if any kind is not covered by the
  activity recorder (the code that logs your clicks and views).
- **Why it fails:** a Web Worker can only send results back through `message` events. The page
  now listens for `message`, `error` and `messageerror` in
  [infra/upload-preparer.js](../../frontend/infra/upload-preparer.js), and the worker listens for
  `message` in [workers/prepare-upload.js](../../frontend/workers/prepare-upload.js). These are
  not user actions, so recording them would be wrong.
- **Proposed fix:** allow those three kinds only on a receiver named `worker` or `self`, the same
  way the test already allows the upload request's own events on a receiver named `xhr`. This
  loosens a committed test, so it needs your approval.
- **I rejected** setting the handlers as properties (`worker.onmessage = …`) to slip past the test's
  pattern. That would hide the listeners from the check rather than satisfy it.

### Q3. May the sessions-table title test look only at the Sessions title?

- **The test:** [screen-flow.spec.js:558](../../e2e/screen-flow.spec.js#L558) checks that
  `#convDetail .table-title` reads "Sessions".
- **Why it fails:** the plan's §4 added a Files table to an open conversation, titled "Files" with
  the same style class. The check now finds two titles, and Playwright refuses an ambiguous match.
- **Proposed fix:** check the first title, or the title above the sessions table specifically.
  The page's behaviour is as planned; only the test's selector is too broad.

### Q4. May the show-switches test wait for Claude's replies?

- **The test:** [views.spec.js:599](../../e2e/views.spec.js#L599). At line 627 it ticks "Claude's
  replies" and counts reply rows *immediately*, without waiting:
  `expect(await replies.count()).toBeGreaterThan(0)`.
- **Why it fails:** under the plan (§5), replies are no longer in the page. Ticking the box asks
  the server for them, so they arrive a moment later. The test passes only when the server
  answers before Playwright counts.
- **Proposed fix:** `await expect(replies).not.toHaveCount(0)`, which waits up to 5 seconds.

### Q5. Should reviews on pruned messages be kept?

- **Observed:**
  - The raw export holds 561 reviews (your `_claude_timeline_user` marks).
  - The processing before this work stored 538.
  - The new processing stores 536.
- **What I found, counting the raw file with Python:**
  - 27 reviewed messages are off the path to their conversation's latest message, so pruning does
    not keep them on that conversation.
  - 22 of them are in "Starting a government contracting business". The server stored 118
    conversations from the file's 117, so one branch was most likely kept as a conversation of its
    own, and these 22 are probably in it. I inferred this from the counts; I did not check the
    stored rows.
  - The other 5 are in branches pruned to notes, and their reviews are not kept anywhere.
  - All 27 reviews say "none of the three flags".
- **Not traced:** why the difference from the old 538 is exactly 2. The old processing dropped 23
  reviews along with the repeated sends it removes, and the two removals overlap in ways I have not
  worked out. The cheapest test: compare stored reviews per conversation against the raw file.
- **Question:** should a review on a message that pruning drops be kept, for example on its note?
  Today it is lost.

### Q6. Should clicking a flag icon filter Review to that flag?

- **Before this work:** clicking a flag icon opened Review on the session with the flag menu set
  to "All", with the flagged messages highlighted.
- **Now:** the page agent made it set the flag menu to the clicked flag, so only flagged messages
  show. Its new page unit tests expect this.
  - The older browser tests expect highlighted rows. With [B5](#b5-review-no-longer-highlighted-a-sessions-rows),
    both now hold: the filter is set *and* the rows are highlighted.
- **Question:** keep the filter, or go back to "All" with highlights? Going back means changing the
  agent's new unit tests in [timeline-view.test.js](../../frontend/tests/timeline-view.test.js),
  which needs your approval.

### Q7. Per-function limits on which fields each server function may change

- **The plan:** §3 says each function's AWS permission should name the row fields it may change.
  The scan's function may change automatic flags only; flag saves may change your flags only.
- **Not done, because it would separate nothing:** one function (the API) runs both the scan and
  your flag saves, and processing writes your reviews. Per-function field limits would give the
  API both sets of fields anyway.
- **What does protect them today:** the code's types, in two port traits. `AutoFlagWriter` can
  write only automatic flags, and `UserFlagWriter` only yours.
- **Question:** accept that, or split the scan into its own function so the permission limit
  means something? Splitting adds a function and its deployment cost.

### Q8. Approve or reverse the test changes listed under T

Each T item below is a change I made to a committed test without your prior approval. Please
answer each as approve or revert.

---

## Test changes Claude made

§10b of the plan lists the test changes you approved. This section lists everything else, plus
approved changes where carrying them out took judgement. Rule-of-thumb references: "committed
test" means in git before the change.

### T1. A new page unit test was changed to expect the opposite of what it first checked

- **File:** [page-flow-timeline.test.js:124](../../frontend/tests/page-flow-timeline.test.js#L124).
- **Before:** it asserted that a failed read-back after saving details stays in the loading box.
  The page agent wrote it; I committed it in `2c59282`.
- **After:** it asserts that the loading box closes and Describe shows "Could not read your
  files' details: …", and the address stays where it was. Changed in `c4ba263`.
- **Why:** it contradicted the older committed browser test (see [B6](#b6-a-failed-read-back-after-saving-a-files-details-left-describe)).
  One of the two had to change, and the older one records behaviour you had approved.

### T2. A new page unit test's stand-in worker gained an optional delay

- **File:** [upload-flow.test.js:20](../../frontend/tests/upload-flow.test.js#L20).
- **Change:** the fake worker used to answer at once (`setImmediate`). It now answers after
  `workerDelay(file)` milliseconds, which is 0 unless a test sets it. Existing tests behave as
  before.
- **Why:** to write the test for [B7](#b7-a-batchs-files-could-be-sent-out-of-order), which needs
  one file to finish preparing later.

### T3. The structure test gained a "workers" layer

- **File:** [structure.test.js:64](../../frontend/tests/structure.test.js#L64).
- **Change:** a `workers` layer may now import `core` and files under the repo's `vendor/` folder,
  which are checked to exist. One same-layer import is allowed (`workers/prepare-upload.js` →
  `workers/slim-stream.js`). Every existing check is kept.
- **Who:** the page agent, as I instructed.
- **Why:** the plan put slimming in a Web Worker using a vendored parser, and the test knew no such
  layer.

### T4. The flag round-trip test now uploads its message first

- **File:** [app.rs:219](../../backend/timeline-api/tests/app.rs#L219),
  `patch_then_get_flags_round_trips_through_real_http_requests`.
- **Change:** it uploads a one-message export before saving flags on that message. The save's
  reply now also carries the recounted session, so the later `GET …/flags` is compared with the
  reply's flag fields only, not with the whole reply.
- **Why:** flags now live on the message's row. A save on a message that was never stored is
  correctly refused, so the old test, which saved flags on a made-up message, could not pass.

### T5. The template test now expects PUT among the allowed methods

- **File:** [template_frontend_url.rs:110](../../backend/timeline-api/tests/template_frontend_url.rs#L110).
- **Change:** the expected CORS method list went from `[GET, POST, PATCH]` to
  `[GET, POST, PATCH, PUT]`.
- **Why:** [B10](#b10-the-api-template-never-routed-put-so-editing-details-could-not-work-on-aws).

### T6. The export test no longer follows a download address

- **File:** [export.rs:21](../../backend/timeline-api/tests/export.rs#L21),
  `export_embeds_the_auto_flags_that_detection_computed`.
- **Removed:** the assertions that `GET /export` answers with an `export_url` starting
  `/_dev/local-storage/get/export/`, and the download of that address.
- **Now:** the test reads the file's text from the parts of the reply and keeps its checks on the
  automatic flags.
- **Why:** the download now comes in the reply itself, in parts (choice
  [O6](#o6-the-annotated-download-comes-inside-the-reply-in-parts)), so there is no address to
  follow. §10b listed `export.rs` for setup changes only.

### T7. Literal upload-progress values gained an empty processing field

- **Files:** [s3_trigger_progress.rs](../../backend/timeline-api/tests/s3_trigger_progress.rs) and
  [upload_progress_contract.rs:45](../../backend/timeline-storage/tests/support/upload_progress_contract.rs#L45),
  six places in all.
- **Change:** each expected `UploadProgress { … }` value gained `processing: None`.
- **Why:** the type gained a field for how far processing has got (§8b). An absent value means
  "not reported", exactly as before.

### T8. The processing log-line test checks two more facts

- **File:** [run_log_lines.rs:121](../../backend/timeline-api/tests/run_log_lines.rs#L121).
- **Approved in §10b:** each step's duration.
- **Also added by me:** `bytes_plain` (the file's size once decompressed) and `totals` (the user's
  counts recounted after the upload). Those facts are new in the log line, and the test checks
  everything the line carries.

### T9. A test not named in §10b reads conversation records from inside the reply

- **File:** [lambda_events.rs](../../backend/timeline-api/tests/lambda_events.rs).
- **Change:** it reads `["conversations"]` from the `GET /conversations` reply, instead of
  expecting the reply to be a bare list.
- **Why:** §10b approved this change for "every test that reads `GET /conversations`" but listed
  six files, and this seventh one was not among them.

### T10. A storage-settings test was renamed

- **File:** [storage_settings.rs](../../backend/timeline-api/tests/storage_settings.rs).
- **Change:** `the_three_storage_names_are_read_without_any_cognito_settings` became
  `the_two_storage_names_…`. Its check of the message-flags table name was removed.
- **Why:** that table no longer exists. §10b approved "six settings become five"; the rename
  follows from it.

### T11. Test doubles gained the new port method (setup only)

- **What:** the hand-written test stand-ins in
  [failed_upload.rs](../../backend/timeline-api/tests/failed_upload.rs) and
  [s3_trigger_progress.rs](../../backend/timeline-api/tests/s3_trigger_progress.rs) gained
  `record_processing_progress`, because the trait they stand in for gained it.
  - In `failed_upload.rs` it is marked `unreachable!`, since that test never calls it.
- No assertion changed.

### T12. Five replacement tests do not name the tests they replace

- **The rule:** the plan's table ("Every removed test and the backend test that replaces it")
  says each replacement names, in its doc comment, the removed test it replaces.
- **What I checked:** every row of that table against the repository. The replacements exist and
  are named for every row except these five removed page tests from `export-format.test.js`:

| Removed | The test that covers it | Named? |
|---|---|---|
| L6 "FORMAT_VERSION marks files this page saved" | the format-version check inside the re-upload test, [processing_rows.rs:727](../../backend/timeline-api/tests/processing_rows.rs#L727) (that test names L16 only) | No |
| L41 "a message without a timestamp is counted but not placed" | `messages_of_unknown_time_are_found_through_their_session_not_a_day` ([review.rs:303](../../backend/timeline-api/tests/review.rs#L303)) and `messages_of_unknown_time_are_kept_and_counted` ([processing_rows.rs:159](../../backend/timeline-api/tests/processing_rows.rs#L159)) | No |
| L48 "embedded automatic and confirmed flags are read from their separate fields" | `reviews_embedded_in_an_upload_are_stored_as_yours` ([processing.rs:238](../../backend/timeline-api/tests/processing.rs#L238)), **pre-existing** | No |
| L74 "a review field with no flag stated is not counted" | `an_empty_review_is_not_stored` ([processing.rs:262](../../backend/timeline-api/tests/processing.rs#L262)), **pre-existing** | No |
| L84 "each conversation's id and its count of untimed messages" | `messages_of_unknown_time_are_kept_and_counted` ([processing_rows.rs:159](../../backend/timeline-api/tests/processing_rows.rs#L159)) | No |

- **The gap:** L48 and L74 are covered by tests that existed before this work. The table promised
  *new* tests, and none was written for them. I have not checked whether
  `reviews_embedded_in_an_upload_are_stored_as_yours` asserts that `_claude_timeline_auto` is
  never taken as a review, which L48 required.
- **Proposed fix:** add the naming comments, and write the two missing tests. I have not done it
  yet.

### T13. Approved browser-test changes: how each was carried out

All are in commit `22b1d15` and approved by §10b. Listed so you can check that the way each was
done matches what you meant.

- **The scan's request shape** ([activity.spec.js](../../e2e/activity.spec.js)): the "first page"
  and "second page" steps now look for scan requests with `part: 0` and `part: 1` and status 200,
  instead of `offset`/`limit`.
- **Confirming a flag persists through a reload** ([upload-flow.spec.js](../../e2e/upload-flow.spec.js)):
  it records the approved message's id, re-uploads the same file, and checks that this message's
  row in Review says "Reviewed". This replaces the old "Loaded N of your confirmed flags" line.
- **Reading the timeline without a stated size** ([upload-flow.spec.js](../../e2e/upload-flow.spec.js)):
  rewritten as approved, and renamed "the timeline's records and sessions, answered without a
  stated size, show what has arrived".
  - `GET /conversations` and `GET /sessions` are sent through a small local server that streams
    each answer in 256-byte pieces with no size header.
  - The test checks the browser saw no size, and that the bar showed "Receiving your
    conversations — N of M" and "Receiving your sessions — N of M".
- **Review search, filters and paging** ([views.spec.js](../../e2e/views.spec.js)): rewritten
  around a made-up export with 130 messages of yours, of which 7 mention "pineapple" and 5 shout.
  - It checks the count becomes exactly "130 messages" (no "at least"), 50 rows, and "Page 1 of 3".
  - An impossible search gives 0 rows; "pineapple" gives exactly 7 rows, each containing the word.
  - "Any flag" gives between 1 and 129 rows, each with a ticked flag.
  - Pages 2 and 3 hold 50 and 30 rows, and going back returns the same first row.
  - The old version read the count while it still said "at least 9", and failed.
- **The annotated download** ([views.spec.js](../../e2e/views.spec.js)): the upload now ticks the
  scan, because the download writes automatic flags only for scanned messages. The test also
  checks:
  - every downloaded message is in the uploaded file, with the same text pieces;
  - the message with files keeps both file names ("Telotrope CoI.pdf", "CP_575_A.pdf").
- **An approved flag in the download** ([views.spec.js](../../e2e/views.spec.js)): also checks it
  is the approved message, with all three flags false and no automatic flags.
- **A session crossing midnight** ([views.spec.js](../../e2e/views.spec.js)): the second message
  now shouts and the scan is ticked. The test checks both days' bars show the same flag icons, and
  at least one.
- **Reading the upload list** ([screen-flow.spec.js](../../e2e/screen-flow.spec.js)): the helper
  that reads `GET /uploads` follows every cursor and collects `uploads` from each part.
- **Not changed:** `waitForAnalysis` and the two analysis tests §10b named. They pass as they are,
  since the helper already waits for the progress bar to go away.
- **The browser-test server's time limit:** the plan approved a shorter limit "so the scan takes
  several requests". [backend-server.js](../../e2e/backend-server.js) sets 20 rows per request for
  *every* request, not only the scan, so all the page's carry-on loops are exercised. That is
  broader than the plan's wording.

### T14. New tests added (not changes, listed for completeness)

- **Backend:** many new test files, including [parts.rs](../../backend/timeline-api/tests/parts.rs),
  [review.rs](../../backend/timeline-api/tests/review.rs),
  [processing_rows.rs](../../backend/timeline-api/tests/processing_rows.rs) and
  [routes_in_parts.rs](../../backend/timeline-api/tests/routes_in_parts.rs). New tests were also
  added inside existing files: [errors.rs](../../backend/timeline-core/tests/errors.rs) and the
  object-store and upload-progress contracts.
- **Page:** 28 new test files by the page agent, plus mine:
  - in [review-view.test.js](../../frontend/tests/review-view.test.js): "a page request replaced by
    a newer one…" and "a session opened from elsewhere flashes its rows…";
  - in [upload-flow.test.js](../../frontend/tests/upload-flow.test.js): "files are sent in their
    order…".
- **Browser:** "the scan's bar and Review's count grow part by part" in
  [views.spec.js](../../e2e/views.spec.js).

---

## Choices Claude made where the plan was silent

Each is in effect now and can be changed. "(page agent)" marks choices made by the second agent.

### O1. Review's time order is exact, by walking groups of overlapping sessions

Review lists messages by time across conversations. Sessions are stored by conversation and
start, and two conversations' sessions can overlap in time. So the server reads sessions in
*groups* whose time spans overlap, reads each group whole, and sorts its messages by time, then
conversation, then id.
- A Calendar-day view instead walks conversations by name.
- A cursor names a group's first session and the last message done.

### O2. What a request for Review messages carries

- **The page sends:** the cursor, how many messages matched so far, how many rows it wants, and
  whether to keep counting after the rows are filled.
- **The server answers with:** rows, the match count, the starting cursors of any new pages of 50
  it passed, and the next cursor.

### O3. Messages of unknown time match a session's span, never a Calendar day

- **Session or analysis span:** a message of unknown time matches it through the session it
  belongs to.
- **Calendar day:** it never matches, since its day is unknown.
- **A conversation placed by its start and end:** every message matches any span overlapping that
  session.

### O4. Notes appear in Review only with the flag menu on "All" and no search

A note marks where a replaced branch was pruned. It has no flags and no text to search, so it
shows only when nothing is being filtered.

### O5. A new "USER" row holds your data version and totals, in the conversations table

- **The plan:** it said these go on "the user's existing record", but no such record existed.
- **What I did:** created a row with sort key `USER` in the conversations table. I did not use the
  unused UsersTable, because no code reads that table, and processing would have needed a new
  setting and a new permission for it.

### O6. The annotated download comes inside the reply, in parts

Each `GET /export` reply carries one part of the file as text, at most 4 MB, plus that part's flag
handles. The page joins the parts and saves the file. It no longer goes through a temporary S3
object and a download address. See also [T6](#t6-the-export-test-no-longer-follows-a-download-address).

### O7. Which files are kept from a conversation

Kept:
- files Claude *presented* (`present_files`);
- widgets;
- your attachments.

Not kept: a file Claude created but never presented.

### O8. A branch kept as its own conversation is named in UTC

The name is "{name}: earlier branch from 2026-06-21 16:38 UTC". The server doesn't know your time
zone.

### O9. Your data version and totals are recounted after each upload, not added to

Recounting means a retried or repeated upload cannot push the totals off. See
[`record_totals`](../../backend/timeline-storage/src/dynamo/user_record_rows.rs#L94).

### O10. One step of the time limit is one row read

Tests count steps instead of time. Each row read, Claude's replies included, is one step. When a
session ends exactly at the limit, the cursor names the next session with nothing yet done in it.

### O11. Processing writes 8 conversations at a time

See [processing.rs:60](../../backend/timeline-api/src/processing.rs#L60). Each conversation's rows
are written in parallel batches. The per-step durations in the processing log line are therefore
summed over conversations, and can exceed the run's wall-clock time.

### O12. The download writes a message's parent only when known

A downloaded file uploaded again therefore keeps its conversations whole, instead of being pruned
by guessed parents.

### O13. A pruned branch's notes can form a session with no messages

Notes count as activity (§4d). Notes of a replaced or revived path far in time from other activity
therefore form a session of their own, with 0 messages.

### O14. A conversation with any message of unknown time is not pruned

See [B3](#b3-a-conversation-with-a-message-of-unknown-time-lost-that-message). Its latest message
can't be determined.

### O15. A revival must replace something

See [B1](#b1-a-message-from-a-later-file-could-be-mistaken-for-a-revived-branch).

### O16. `GET /export` still issues flag handles

A *flag handle* is a signed token that lets a flag save prove it names a real message. §10b's
wording ("`GET /export` still does too") was taken literally.

### O17. flate2's compression backend is zlib-rs

The gzip library flate2 (MIT or Apache-2.0) uses zlib-rs, under the Zlib licence. That licence is
permissive, but it isn't one of the four your rule names (MIT, BSD, Apache-2.0, ISC). Please
confirm it is acceptable.

### O18. A test hook in the page: `?upload=unslimmed` sends files unslimmed (page agent)

See [`sendsUnslimmed`](../../frontend/ui/load-flow.js#L73). It exists so the 2 MB browser test can
reach the server's size limit. A user who adds it to the address gets the old, slow upload. Files
the worker can't slim are also sent as they are, and the server then reports what is wrong.

### O19. Slimming keeps the file's outer shape (page agent)

A bare list stays a list, and a saved timeline file stays wrapped with its format version.
- **Why:** the server removes repeated sends only for bare lists.
- **What it drops:** `_claude_timeline_auto`.
- **What it keeps:** the fields the server reads. It never adds a field.

### O20. A part holds at most 1,000 records or 2,000 sessions

See [conversations.rs:37](../../backend/timeline-api/src/routes/conversations.rs#L37) and
[sessions.rs:28](../../backend/timeline-api/src/routes/sessions.rs#L28). The time limit alone would
let a user with very many conversations get one answer over AWS's 6 MB limit. At these caps a part
is about 1 MB.

### O21. After a flag save, only that message's session is refreshed (page agent)

- **What the page does:** it takes the counted session from the save's reply and asks for Review's
  current page again from its starting cursor, counting again.
- **What it does not do:** read all sessions again.

### O22. The timeline is read when Describe is left, not while it is open (page agent)

Describe can change a conversation's start and end, which moves its sessions. So the timeline is
read after Describe closes, and again after details are saved.

### O23. Review loads only when shown (page agent)

- **When it loads:** on a tab click, an address or an entry point, not when the timeline opens.
- **Search:** waits for a 300 ms pause in typing.
- **Short parts:** when a part arrives with fewer than 50 rows, the page keeps asking until the
  page is full.
- **Back and Next:** enabled only for pages whose starting cursor is known.

### O24. A Calendar day is sent as local midnight to the millisecond before the next midnight (page agent)

The server's span includes both ends, so ending at the next midnight would include its messages.

### O25. The "may have been changed later" mark follows the server's rule (page agent)

A command marks every earlier file creation whose path or file name it mentions. This matches
[kept_files.rs](../../backend/timeline-core/src/kept_files.rs).

### O26. One shared progress bar component (page agent)

`createProgressBar` in [status-indicators.js](../../frontend/ui/widgets/status-indicators.js) is
used for the load screen, Review, Analytics, the annotated download and a conversation's files.
The processing wait keeps its own clock wording and adds bytes read, then conversations written.

### O27. The new bars record nothing in the activity log (page agent)

Review's, Analytics', the download's and the files' bars show progress but log no "shown" events.
- **Why:** the list of places that may log is fixed by a committed test, `SHOWN_PLACES` in
  [activity-event.test.js:160](../../frontend/tests/activity-event.test.js#L160).
- **The load bar:** it logs each of its new messages once.
- **To log the new bars too:** that test would have to change.

### O28. Links between a note and the conversation it became (page agent)

A note's "kept as" link, and a conversation's details, link to `#conversations/<index>`. The
Conversations tab shows "kept as its own conversation" links in both directions.

### O29. Opening a listed file opens Review on its message (page agent)

Review opens on the session holding the message. Claude's replies are turned on if Claude
presented the file, and the row is highlighted.

### O30. Source-map lines removed from the vendored parser (page agent)

Each `//# sourceMappingURL=` line was deleted from
[vendor/streamparser-json/](../../vendor/streamparser-json/).
- **Why:** the maps are not vendored, and Node's coverage report fails on a file that names a map
  it can't read.
- **Recorded:** in that folder's README, with checksums taken after the change. The vendored files
  are therefore not byte-identical to the published package.

---

## Problems found

### P1. Node 18 is the shell's default; the page's tests need Node 20

`node` on this machine's default PATH is v18, under which one page test fails (it needs the global
`crypto`). The package asks for Node 20. All runs used
`~/.nvm/versions/node/v20.20.2/bin`. Nothing was changed on the machine.

### P2. The disk filled during the work

`backend/target/` had grown to about 30 GB. I deleted `target/debug` and the coverage build
directory, and they were rebuilt as needed. 8.3 GB was free at the last check, and the deploy
script needs 3 GB.

### P3. Another timeline-api is listening on port 3000

The browser tests and the measurements used their own ports (3123, and 3917/8917). I did not touch
or identify the server on 3000. If it is yours from an earlier session, it is running code of
unknown age.

### P4. Local timing quirks noted in the measurement analysis

These are explained in [the measurement analysis](2026-10-06-load-only-what-the-page-shows-measurements.md),
and listed here so you can track them by ID:
- **P4a:** locally, processing happens inside the upload request, so the page cannot time it
  separately.
- **P4b:** the local scan slowed from 291 ms to 404–414 ms as other users' rows accumulated in the
  in-memory store, and returned to 265 ms after a restart. The page-side part of the variation is
  not traced.

### P5. The dev stack and its committed settings disagree

See [Q1](#q1-how-to-deploy-the-dev-stack-which-today-serves-the-public-address).
- **The disagreement:** the deployed stack still has the public address, while
  [infra/samconfig.toml](../../infra/samconfig.toml) says otherwise.
- **The deployed parameters:** they also include `RecordActivity=on` and `LogRetentionDays=7`.
  - `RecordActivity` defaults to "on" in the template, so it would not change.
  - `LogRetentionDays` was not compared with the template's default.

### P6. The live public page will break whenever the backend is deployed alone

The page at howangryami.telotrope.ai asks for the old timeline download on load. Any deployment
of the new API without publishing the new page to the same address breaks it. `scripts/deploy.sh`
publishes both together, so this risk applies only to a partial or manual deployment.
