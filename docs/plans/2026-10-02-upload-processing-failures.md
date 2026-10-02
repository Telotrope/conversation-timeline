# Upload processing: failures that reach the page, and a wait you can see

**Status:** proposed 2026-10-02, not approved.

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

- **The server records each attempt.** At the start of every attempt `process_upload` adds one to
  an `attempts` count on the upload's row and records the time (one `UpdateItem` with `ADD`, so a
  retry can't lose a count). When an attempt fails with a storage error it also records that
  error's message before returning it for AWS to retry. This adds a persisted "in progress" state,
  which the port's documentation
  ([uploads.rs:1-10](../../backend/timeline-core/src/ports/uploads.rs#L1-L10)) says doesn't
  exist "because nothing currently reads one"; now the page reads it, so the documentation
  changes with it.
- **`GET /uploads/{id}` says more while processing:** `{"status":"processing","attempt":2,
  "max_attempts":3,"last_error":"item not found"}`; before the first attempt starts (the file is
  still landing, or AWS hasn't started the function), `{"status":"waiting"}`. `max_attempts`
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
  server is still processing.

**Tests:** `process_upload` with the in-memory stores: the count goes up once per attempt, a
storage error is recorded as the last error, and `Ready`/`Failed` replace both. The status route
for each state. `upload-wait.js` unit tests for each message and the 10-minute message. A browser
test with the stand-in server answering waiting → processing → retrying → ready, checking each line
appears and the bar moves (its animation is running).

## 4. Audit: every wait and every background failure

The user asked whether this pattern is elsewhere. The audit covers **all** of the following, not a
sample, and is written to `docs/analysis/2026-10-02-error-reporting-audit.md` with, for each item,
the code it reads (file and line), what the user sees on failure, and a verdict: reported
truthfully / reported falsely / not reported / waits without limit.

- **Every place the page waits on the network:** sign-in and the code exchange, loading the deploy
  configuration, `POST /uploads`, the `PUT` to S3, the status polling, the detection pass, `GET
  /export`, the export download, flag saves, and anything else `frontend/` `await`s on `fetch`.
- **Every server path whose error goes only to a log:** both existing Lambda binaries' `main`
  functions, `s3_trigger.rs`, and every `eprintln!` in `timeline-api` and `timeline-storage`.
- **Every write followed by a read** in `timeline-storage/src/dynamo/` (§1's bug class).
- **Every progress message**, for whether it can show while nothing is happening.

Items found broken are fixed under this plan if the fix is small and the same shape as §1–3;
anything larger is listed in the analysis with its own proposed fix, and comes back to the user
before it's built.

## 5. Rollout

1. Build §1–3 and the audit; all suites pass; `scripts/check-template.sh` passes.
2. The user redeploys (`sam deploy`; the new function is created, nothing is replaced) and uploads
   the same export again.
3. **Measured, not assumed:** the processing log shows one attempt and no `item not found`;
   whether the page's lines appeared as described. One clean run does not prove §1's cause was
   right (C2).

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

### C6 [RESOLVED]: The page's bar never moved
`setLoadProgressIndeterminate` fills the bar to 100% and stops, which reads as finished or stuck.
**Resolution:** §3 replaces it with a moving bar and an elapsed-time line; see
[§3 (line 90)](2026-10-02-upload-processing-failures.md#L90).
