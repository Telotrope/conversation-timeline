# Upload processing: failures that reach the page, and a wait you can see

**Status:** approved 2026-10-02, with the order changed by the user: the error handling (§1a,
§2, §3) is built and deployed first, **without** §1's fix, so it can be tested live against the
real intermittent failure; then §1, §4, and optionally §6. See §5.

## Why

On 2026-10-02 the user's first upload to the deployed stack (60.6 MB) sat at "Processing on the
server…" for about four minutes with no sign of life, then finished. The processing function's log
(read with `aws logs filter-log-events`, times UTC):

| Time | Event |
|---|---|
| 15:21:47 | the file lands in S3 |
| 15:22:07 | attempt 1 fails after 6 s: `processing "raw/…/fdb77041-….json" failed: item not found` |
| 15:23:10 | attempt 2 (AWS's automatic retry, 63 s later) fails the same way |
| 15:25:29 | attempt 3 (AWS's last retry, 139 s later) succeeds in 6 s |

Afterwards the flags table held 538 rows and the conversations table 118. Three problems, all
observed in that run:

1. **An intermittent storage failure.** `"item not found"` is `StoreError::NotFound`, which the
   DynamoDB adapters produce in one place:
   [message_flags_table.rs:276-278](../../backend/timeline-storage/src/dynamo/message_flags_table.rs#L276-L278),
   where `set_user_flags` writes a review and reads the row straight back with `get`
   ([line 170](../../backend/timeline-storage/src/dynamo/message_flags_table.rs#L170)), which
   doesn't ask for a strongly consistent read. DynamoDB's default read may briefly miss a row just
   written. *Inferred, not proven:* the read-back missing a new row is the best fit (two attempts
   failed partway through rows the third got past), but no log line names the row.
2. **A failure that every retry hits would never reach the page.** By design
   ([s3_trigger.rs:7-15](../../backend/timeline-api/src/s3_trigger.rs#L7-L15)) a storage error
   isn't recorded, so AWS can retry. Nothing records anything after the last retry, so the upload's
   status stays "processing" forever. The page gives up after 10 minutes
   ([upload-wait.js:37-39](../../frontend/core/upload-wait.js#L37-L39)) saying "still
   processing … try reloading later", which would be false. The same is true when the function is
   killed by its 300-second time limit or by running out of memory: our code never runs to record
   it.
3. **The wait looks the same as a hang.** The page shows one fixed sentence and a progress bar that
   `setLoadProgressIndeterminate`
   ([status-indicators.js:41-45](../../frontend/ui/widgets/status-indicators.js#L41-L45)) sets to a
   static 100% width: no movement, no elapsed time, no hint that the server hit an error and is
   retrying. The user couldn't tell retrying from hung, and says a user would likely give up.

The user also asked for the error handling to be checked more widely, since this kind of failure
"could turn up anywhere later" (§4).

## 0. Experiment first: does a default read miss a row just written? (done before §1, at the user's request, 2026-10-02)

- **What:** a test in `backend/timeline-storage/tests/dynamo_read_after_write_experiment.rs`,
  marked `#[ignore]` so ordinary test runs skip it; run on demand with
  `cargo test -p timeline-storage --test dynamo_read_after_write_experiment -- --ignored --nocapture`
  after `aws login`, with `TIMELINE_EXPERIMENT_TABLE` naming the deployed flags table.
- **How:** 600 new rows under one scratch partition key (`experiment#<run id>`), each written with
  the same kind of request `set_user_flags` sends (`UpdateItem … SET user_caps = :v`) and read back
  immediately. Rows alternate between a default read and a strongly consistent read, so both kinds
  see the same conditions. It counts misses for each kind, records each write's and read's time,
  and prints both. It then deletes every row it wrote, and fails if any delete fails.
- **Reading the result:** misses on default reads and none on strongly consistent reads support
  the inferred cause. No misses in either is **not** a refutation: from this machine each request
  takes longer than from inside Lambda, giving DynamoDB more time to catch up; the printed times
  show by how much. Misses on strongly consistent reads would mean the cause is something else.
- **Dependency:** `aws-config` 1 (Apache-2.0, already used by `timeline-api`) as a dev-dependency of
  `timeline-storage`, to load the `aws login` credentials.
- Results go in `docs/analysis/2026-10-02-read-after-write-experiment.md`.

### 0b. The same experiment from inside Lambda (chosen by the user 2026-10-02 after §0 was inconclusive)

§0 ran from `dev`, about 40 ms from DynamoDB per request, and found 0 misses in 300 default reads
([analysis](../analysis/2026-10-02-read-after-write-experiment.md)). Production reads arrive much
sooner after the write. This runs the same loop where production runs.

- **A new workspace crate, `backend/timeline-experiments/`**: a library function
  `read_after_write::run(client, table, rows) -> Report` (the §0 loop: write a new row under
  `experiment#<run id>` with `set_user_flags`'s kind of request, read it straight back,
  alternating default and strongly consistent reads, time both; then delete every row) and a
  Lambda binary, `read_after_write`, that runs it with the row count from its input and returns
  the report as JSON. Errors are returned in the report with the row number, never dropped.
  Dependencies are ones the workspace already uses: `aws-config`, `aws-sdk-dynamodb`,
  `lambda_runtime`, `tokio`, `serde`, `serde_json`, `uuid` (all MIT or Apache-2.0).
- **Its own small stack, separate from `timeline-dev`**: `infra/experiments/read-after-write.yaml`,
  stack `timeline-experiment-read-after-write`. One function: ARM, 512 MB (the processing
  function's size, so the same CPU share), 300 s limit, allowed only `UpdateItem`, `GetItem` and
  `DeleteItem` on the flags table, named by a parameter. Built with
  `cargo lambda build --release --arm64 -p timeline-experiments`.
- **Run:** Claude deploys the stack, invokes it once with 4,000 rows
  (`aws lambda invoke`), records the report in the §0 analysis document, and deletes the stack.
  The crate and template stay in the repository so the run can be repeated.
- **Tests:** `run` against DynamoDB Local (the storage crate's existing helper, included by path):
  every row is read back, the report counts both kinds, and nothing is left in the table; against a
  table that doesn't exist, the report carries the first write's error and row 0.
  `scripts/check-template.sh` is extended to also check the experiment template.
- **Reading the result:** as §0. In addition, the report's median request time shows how close
  this comes to production's timing.

## 1. Write-then-read without the read

`set_user_flags` asks DynamoDB to return the row as it is after the update, in the same request
(`UpdateItem` with `ReturnValues = ALL_NEW`, `aws-sdk-dynamodb`, Apache-2.0, already a
dependency), and builds the `MessageFlagRecord` from that, so there is no separate read to miss the
row. When the review sets nothing (`user_update_expression` returns `None`), there is no write to
return a row from; that path keeps its `get`, made strongly consistent (`consistent_read(true)`).
The same change is made to any other write-then-read in the DynamoDB adapters that §4's audit
finds.

**Tests:** DynamoDB Local always reads its own writes, so it can't show the bug. The adapter tests
capture the request the SDK sends, using `aws-smithy-runtime`'s `capture_request` test client
(Apache-2.0; checking it's available in the version we use is the first step), and check that the
`UpdateItem` asks for `ALL_NEW` and that the `None` path's `GetItem` asks for `ConsistentRead`. The
existing DynamoDB Local tests keep checking the results are right.

## 1a. Errors name the row they were working on

`"item not found"` named no row, which left the cause unprovable (C2). `process_upload` wraps
each store error from its review and summary loops in a new `ProcessingError` variant naming what
it was doing, e.g. `saving review 37 of 538 (conversation <id>, message <id>): item not found`
and `saving conversation summary 5 of 118 (conversation <id>): …`. Existing variants are
unchanged, so the committed tests that match them are untouched. **Tests:** a failing flag writer
and a failing summary store each produce the new message with the right numbers and ids.

## 2. Every processing failure ends as "failed" on the page

- **What the page needs to hear, and when:** a file that can't be processed after the last retry
  becomes `failed` with a reason, whatever the cause: our error, the time limit, or running out of
  memory.
- **How: an "on failure" destination.** AWS can hand an asynchronous invocation that failed its
  last retry, with the error, to another function (the function's `EventInvokeConfig` →
  `DestinationConfig.OnFailure` in the template). This catches time limits and out-of-memory
  kills too, which nothing inside our function can. The template sets
  `MaximumRetryAttempts: 2` explicitly (today's AWS default, written down so §3 can count on it)
  and points `OnFailure` at a new small function.
- **The new function, `record_failed_upload`** (a third binary in `timeline-api`, built by the same
  `cargo lambda build`): reads the failed invocation's original S3 notification from the payload
  AWS sends, traces each object key to its upload with the existing `parse_raw_object_key`, and
  records `Failed { reason }` through `UploadOutcomeStore`. The reason is
  `"the server couldn't process the file after 3 attempts: <AWS's error message>"`. Its only
  permissions are writing that table. If it fails itself, it returns the error, which AWS logs;
  nothing retries it further (C4).
- **Its input is a format we haven't captured.** The tests' sample destination payload is written
  from AWS's documentation and marked as such, exactly as the S3 notification sample was (C24 in
  the migration plan); replacing it with a real one is C5 here.

**Tests:** the new handler, through its public function: a payload with one key records `Failed`
with that reason; two keys record both; a key that isn't a raw upload is an error naming it; a
payload that isn't a destination record is an error. A template test (the style of
`template_event_logging.rs`) checks the retry count, the destination, and the new function's
permissions.

## 3. A wait you can see

- **The server records each attempt.** At the start of every attempt, the S3-trigger path
  (`s3_trigger.rs`, the AWS-only caller; the local server processes in one go and needs none of
  this) adds one to an `attempts` count and records the time, on a separate `PROGRESS#<upload id>`
  row of the conversations table (one `UpdateItem` with `ADD`, so a retry can't lose a count; a
  separate row so the outcome row and its committed tests are untouched, and the `CONV#` listing
  never sees it). When an attempt fails with a storage error it also records that error's message
  there before returning it for AWS to retry; if that recording itself fails, it's logged and the
  original error is still returned. New `UploadOutcomeStore` methods: `record_attempt`,
  `record_attempt_error`, `get_progress`, in both adapters. This adds a persisted "in progress" state,
  which the port's documentation
  ([uploads.rs:1-10](../../backend/timeline-core/src/ports/uploads.rs#L1-L10)) says doesn't
  exist "because nothing currently reads one"; now the page reads it, so the documentation
  changes with it.
- **`GET /uploads/{id}` says more while processing:** `{"status":"processing","attempt":2,
  "max_attempts":3,"last_error":"item not found"}`; before the first attempt starts (the file is
  still landing, or AWS hasn't started the function), plain `{"status":"processing"}` as today, so
  the committed route tests keep passing; the page reads "no attempt yet" as waiting. `max_attempts`
  comes from the same setting as the template's retry count, passed to the API function as an
  environment variable, parsed once at start-up.
- **The page shows** a moving bar (a CSS animation, per the project's frontend rules) and, under
  it, one line that updates on every answer:
  - waiting: "Waiting for the server to start — 0:12"
  - first attempt: "Processing on the server — 0:20"
  - a retry: "The server hit an error (item not found) and is trying again automatically: attempt
    2 of 3. AWS waits 1–2 minutes between attempts. — 1:34"
  - The clock counts from when the file finished sending.
- **The 10-minute limit's message tells the truth:** "No answer from the server after 10 minutes.
  Its last status was: <the line above>. Reload later to check again." It no longer claims the
  server is still processing. **Held back until the user approves changing the committed test**
  that pins the old message ([upload-wait.test.js:47-50](../../frontend/tests/upload-wait.test.js#L47-L50));
  see C7.

**Tests:** `process_upload` with the in-memory stores: the count goes up once per attempt, a
storage error is recorded as the last error, and `Ready`/`Failed` replace both. The status route
for each state. `upload-wait.js` unit tests for each message and the 10-minute message. A browser
test with the stand-in server answering waiting → processing → retrying → ready, checking each line
appears and the bar moves (its animation is running).

## 4. Audit: every wait and every background failure

The user asked whether this pattern is elsewhere (rewritten 2026-10-02 at the user's request).
The organizing rule is theirs: **AWS errors must be handled by Rust; Rust errors must reach the
page and be shown.** An AWS error that isn't anticipated and retried propagates to the page. The
place this breaks is wherever Rust runs **outside a request the page made** (a background job),
because there is no response to carry the error.

The audit covers **all** of each step, not a sample, and is written to
`docs/analysis/2026-10-02-error-reporting-audit.md`. Each item gets the code it read (file and
line), what the user sees on failure, and a verdict: *reported truthfully / reported falsely /
not reported / waits without limit*.

1. **Every AWS call in Rust** (each `.send().await` in `timeline-storage` and `timeline-api`): is
   its error retried, passed on, or dropped; and if passed on, where does it end up?
2. **Every server entry point** (each route in `timeline-api/src/routes/`, each Lambda binary's
   handler): does every error path end in an HTTP response with a readable message, or a recorded
   status, rather than only a log line? Every `eprintln!` is checked for what else happens next
   to it.
3. **Every background job** (today the S3-triggered processing; later V3's Bedrock queue): does
   it have a final status the page can read in every case, including after AWS stops retrying?
   Includes every write followed by a read in `timeline-storage/src/dynamo/` (§1's bug class: a
   default read can miss a row created by the write just before it).
4. **Every place the page waits** (each `fetch` and poll in `frontend/`): is every failure shown,
   with a true message, and is there a time limit? Includes every progress or status message, for
   whether it can be shown while nothing is happening or after it has stopped being true.
5. **Failures outside our code** — a function's time limit, running out of memory, a crash at
   start-up, API Gateway refusing a request: for each function, what does the page see? Read from
   the template and the page's error handling; confirmed live where a test setting allows it (the
   `FailProcessing` setting, §2b).
6. **Which of these can become automatic checks**: e.g. a template test that every function AWS
   starts in the background has an "on failure" destination, so a new one can't be added without
   it. Each candidate is listed with what it would catch; none is built without the user's
   approval.

**When it runs:** once, in phase B (§5), across the whole codebase; it has not been run yet. From
2026-10-02 the same questions are also asked of every code addition, before its tests are
written (CLAUDE.md's "post-addition check"), so new code is checked as it's added rather than
only by this one-time audit.

Items found broken are fixed under this plan if the fix is small and the same shape as §1–3;
anything larger is listed in the analysis with its own proposed fix, and comes back to the user
before it's built.

## 3b. The failure message after the last attempt (from the user's live test, 2026-10-02)

The first complete live failure (all 3 attempts hit "item not found") ended with the red text
above the bar holding the whole error, which repeats "the server couldn't process the file", and
the grey text below the bar still showing the last waiting line ("…trying again automatically:
attempt 3 of 3… — 3m 31s"), frozen and no longer true.

- **Red text above the bar is for the user:** a short statement that the file couldn't be
  processed.
- **Grey text below the bar is the full error, for an expert:** every diagnostic detail kept (the
  object key, the review number, conversation and message ids, the store error), not shortened,
  with the repeated phrase removed. It replaces the waiting line, so nothing stale remains.
- **Open, to be decided in this plan later, not now:** how the text above and below the bar is
  used in general, for every step of a load. Today the two often repeat each other.

## 2b. A switch to make processing fail on purpose (built 2026-10-02, at the user's request)

**Status:** built and tested locally (commits `a9d09b1`, `c154618`); not yet run on AWS. The
walkthrough's step 10 ([infra/README.md](../../infra/README.md)) is the live test. Differences
from the design below: the on/off parsing is one helper shared with `LogS3Events`; the setting
reaches the code through new `handle_s3_event_with` / `handle_raw_s3_event_with`, with the old
names kept as wrappers so committed tests are unchanged. Found while writing step 10: after the
last attempt the page's message repeats "the server couldn't process the file" (the page adds it
in front of a reason that already starts that way); see C8.

Live-testing §2 needs all three attempts to fail; with the user's export one attempt fails about
24% of the time, so all three would be rare. Proposed: a template parameter `FailProcessing`,
`off` (default) or `on`, in the style of `LogS3Events`, setting `TIMELINE_FAIL_PROCESSING` on the
processing function only. When `on`, each attempt records its attempt, then fails with a storage
error `"failing on purpose (FailProcessing is on)"` before reading the file. Parsed once at
start-up into an enum; anything else stops start-up. Without it, a live test is still possible by
deleting the uploaded object from S3 in the ~20 seconds before processing reads it (each attempt
then fails with "object not found"), which is fiddly.

## 5. Rollout

**Phase A (error handling, no fix):**
1. Build §1a, §2, §3 (and §2b if approved); all suites pass; `scripts/check-template.sh` passes.
2. The user rebuilds and redeploys (`sam deploy`; the new function is created, nothing is
   replaced) and uploads the export, possibly several times. Expected, about one upload in four:
   the page shows "trying again" with the named row; every upload ends ready or failed, never
   silent. With §2b on: three attempts, then "failed" with the reason, within about 4 minutes.

**Phase B (the fix):**
3. Build §1 and the audit (§4).
4. Redeploy and upload again, several times. **Measured, not assumed:** every upload takes one
   attempt and no log line says `not found`. A few clean runs are evidence, not proof (C2).

**Phase C (optional):** §6, decided after D5 measures where processing time goes.

## 6. Concurrent writes (optional; decided after deployment check D5)

Processing writes 538 reviews and 118 summaries one at a time, each waiting for the previous
reply. Keeping about 16 requests in flight at once (the same requests, so update-in-place,
retry-safety and the error naming in §1a are unchanged) would take the reviews from roughly 1.9 s
to 0.15 s and the summaries from 0.4 s to 0.03 s, estimated from the §0b medians, not measured.
DynamoDB's `BatchWriteItem` is not used for reviews: it can only replace whole rows, which would
erase a row's automatic flags. Worth doing only if D5 shows processing time matters; the
detection pass may matter more.

## Self-critique log

### C1 [RESOLVED]: Recording a failure on every storage error would end a wait that a retry could still win
The simplest fix for §2 is to record `Failed` whenever an attempt fails. In the 2026-10-02 run that
would have shown an error at 15:22:13 for an upload that succeeded at 15:25:35.
**Resolution:** a failed attempt records only the attempt and its error, which the page shows as
"trying again"; `Failed` is recorded only after the last retry, by the destination function. See
[§2 (line 62)](2026-10-02-upload-processing-failures.md#L62) and
[§3 (line 90)](2026-10-02-upload-processing-failures.md#L90).

### C2 [OPEN]: §1's cause is inferred, and the bug can't be reproduced locally
DynamoDB Local reads its own writes, so no local test can show the original failure; §1's tests
only show the request now asks for the right thing. **Mitigation in plan:** the fix removes the
separate read rather than tuning it, so it doesn't depend on the guess being exactly right; and
§2–3 make any remaining failure visible. **Open:** trigger is the rerun in §5: any `item not
found` in the log means the cause was something else.

### C3 [RESOLVED]: A count kept by our own code misses time limits and out-of-memory kills
If the function is killed, the code that would record the failure never runs.
**Resolution:** the "on failure" destination is AWS's, run after any kind of failure; see
[§2 (line 62)](2026-10-02-upload-processing-failures.md#L62).

### C4 [OPEN]: The destination function can fail too
If `record_failed_upload` can't write the table, the upload stays "processing". **Mitigation in
plan:** its error is logged, and the page's 10-minute message now reports the last status it saw
instead of claiming the server is still working. **Open:** trigger is the audit finding any other
path where a failure leaves the page waiting; if one does, the fix may be a page-side rule (e.g.
"no attempt has started within 5 minutes") that covers both.

### C5 [OPEN]: The destination payload sample isn't a real one
The test sample for §2's input comes from AWS's documentation, against the project rule that
communication tests use verified samples. **Mitigation in plan:** marked as unverified in the
fixtures folder's README, like the S3 notification sample before C24. **Open:** trigger is the
first real failure after deployment; or the user can force one (upload a file larger than the
function's memory allows) to capture it on purpose.

### C7 [RESOLVED]: The truthful 10-minute message needs a committed test changed
[upload-wait.test.js:47-50](../../frontend/tests/upload-wait.test.js#L47-L50) asserts the old,
false message. Project rules forbid changing a committed test without approval. **Mitigation in
plan:** Phase A kept the old message at first. **Resolution (2026-10-02):** the user approved
changing the test; message and test now use the plan's wording, see
[§3 (line 177)](2026-10-02-upload-processing-failures.md#L177).

### C8 [OPEN]: The final failure message repeats itself
The page shows `the server couldn't process the file: ${reason}`
([upload-wait.js](../../frontend/core/upload-wait.js)), and `record_failed_upload`'s reason
already begins "the server couldn't process the file after 3 attempts: …"
([failed_upload.rs](../../backend/timeline-api/src/failed_upload.rs)). **Proposed fix:** the
reason becomes "all 3 attempts failed; the last error was: …", which changes the expected text in
the committed `tests/failed_upload.rs`. **Open:** trigger is the user's approval to change that
test.

### C6 [RESOLVED]: The page's bar never moved
`setLoadProgressIndeterminate` fills the bar to 100% and stops, which reads as finished or stuck.
**Resolution:** §3 replaces it with a moving bar and an elapsed-time line; see
[§3 (line 90)](2026-10-02-upload-processing-failures.md#L90).
