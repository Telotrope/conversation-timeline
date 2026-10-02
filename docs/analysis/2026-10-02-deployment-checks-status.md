# Deployment checks: status on 2026-10-02

The checks only a real deployment can do are defined in the migration plan's table
([migration plan, line 1391](../plans/2026-09-09-rust-aws-backend-migration.md#L1391)). This records
where each stands on the `dev` stack, as of its last deploy (2026-10-02 16:47 UTC). Nothing here was
run as a deliberate check unless the table says so; most evidence comes from the user's own uploads
through the page, read afterwards from the functions' logs.

**Headline (updated ~19:35 UTC): seven checks passed (D1, D2, D4, D5, D6, D7, D9); D3 passed in part and failed in part (the command-line script is broken; fix planned, not yet approved).**

## Status of each check

| Check | What it verifies | Status | Evidence |
|---|---|---|---|
| D1 | AWS accepts the template | **Passed** | `sam deploy` succeeded, including the 2026-10-02 deploy adding `RecordFailedUploadFunction` (the user confirmed the change list after Claude checked AWS's reason for each change) |
| D2 | The browser's pre-flight `OPTIONS` request answered without login, from the allowed address only | **Passed** (Claude, 18:54 UTC) | `curl -X OPTIONS .../conversations` with `Origin: https://dev.tail13dce8.ts.net`: 204 with `access-control-allow-origin: https://dev.tail13dce8.ts.net`, allowed methods `GET,PATCH,POST`, headers `authorization,content-type`. With `Origin: http://example.com` and `http://localhost:8000`: 204 with no `access-control-*` headers, so a browser blocks them. The page's own requests work (earlier uploads) |
| D3 | Real Cognito login: hosted page, code exchange, command-line script | **Page part passed; script part FAILED** (user ran it, ~19:00 UTC) | The user signs in through Cognito's page and uploads. `scripts/aws-dev-token.sh` failed before reaching Cognito: `ParamValidation: Error parsing parameter 'cli-input-json': Invalid JSON received`. Cause, found by Claude with a made-up client ID (no login attempted): aws-cli 2.37.8 rejects valid JSON piped through `file:///dev/stdin` ([aws-dev-token.sh:25](../../scripts/aws-dev-token.sh#L25)), but accepts the same JSON from an ordinary file (answering "client does not exist"), and from `/dev/stdin` when it is connected to a file (`< file`). A pipe on standard input does not affect reading a named file. A named pipe (created with `mkfifo`) was read once, its writer finished, and `aws` then hung waiting on it; so `aws` appears to open its input twice, and a reopened pipe is empty (inferred from the hang; `strace` was not available to trace it, and why it opens twice is not known). The script's test uses a stand-in `aws` that never parses the JSON, so it couldn't catch this. Not fixed |
| D4 | The API refuses no token, a bad token, another pool's token | **Passed** (Claude, 18:54 and ~19:10 UTC) | `GET /conversations` with no `Authorization` header, and with `Bearer nonsense`: both 401 `{"message":"Unauthorized"}`. Earlier, an expired token was also refused with 401 (17:53 UTC). Other pool (with the user's approval): Claude created a throwaway pool `timeline-d4-throwaway` (`us-east-1_SKpsHsEdx`) with one user, signed in, and got a real RS256-signed access token with issuer `https://cognito-idp.us-east-1.amazonaws.com/us-east-1_SKpsHsEdx`; the API refused it with 401 `{"message":"Unauthorized"}`. The pool was then deleted (`list-user-pools` afterwards shows only `timeline-users-dev`). Our own pool's tokens are accepted (the page's uploads), so the refusals are not a blanket refusal |
| D5 | ~60 MB export processed within memory and time limits; detection pass pages each under 30 s | **Passed** (user's upload with detection ticked, ~19:10 UTC; logs read by Claude) | Processing ran at 19:10:17 and 19:11:26 (6.6 s, 6.1 s; at most 286 MB of 512 MB; no error lines, no failure recorded). The user saw the detection results finish without an error. Every one of the 55 API requests from 19:09:47 to 19:14:57 took at most 12.0 s (most memory 413 MB of 512 MB), so every detection page was under 30 s whichever requests they were; API Gateway counted no 5xx in that window. Which requests were detection pages is **inferred**: 25 back-to-back requests of 3.0–4.4 s from 19:11:36 to 19:12:56. The API logs nothing naming the request (see "Found during these runs"). Processing ran twice because the user uploaded twice; only the second upload (processed at 19:11:26) had detection ticked (the user's account), which fits the detection-like requests starting at 19:11:36 |
| D6 | The flag-handle secret reaches the API | **Passed** (user, ~19:35 UTC) | After Approve, the save-outcome text beside "Download annotated conversations.json" read "Saved.", which the page shows only for a 2xx answer to the flag save ([api-client.js:188-213](../../frontend/infra/api-client.js#L188-L213)); a handle the API couldn't verify would have been refused with 403. (The row's "Reviewed" label alone wouldn't show this: it changes before the server answers.) The user found the shared, misplaced text confusing; filed as [deferred problem 5](../plans/2026-10-02-deferred-problems.md) |
| D7 | Real tables' keys match the tests' assumptions | **Passed** (Claude, 18:54 UTC) | `aws dynamodb describe-table`: `timeline-conversations-dev` and `timeline-message-flags-dev` both have `pk` (string, HASH) + `sk` (string, RANGE), no secondary indexes, the same layout the test helper creates ([dynamodb_local.rs:166](../../backend/timeline-storage/tests/support/dynamodb_local.rs#L166)). `timeline-users-dev` has `pk` only; no Rust code reads it (a search for its environment variable `TIMELINE_USERS_TABLE` finds only the template), so the tests have nothing to match there. Uploads and review flags also work against the real tables |
| D9 | Start-up time, including downloading Cognito's keys | **Passed** against a limit of 1 s set by the user (2026-10-02) | Lambda's `Init Duration` is the time a fresh copy of a function spends starting before its first request. The API downloads Cognito's keys during that time (read at [main.rs:143-145](../../backend/timeline-api/src/main.rs#L143-L145), before the runtime starts at line 152). Worst measured: API 455 ms, upload processing 123 ms, failure recorder 119 ms; see below |

## Measurements (last 24 hours of logs, read 2026-10-02)

Read from each function's `REPORT` log lines with `aws logs filter-log-events`.

| Function | Runs | Duration | Most memory used | Start-ups and start-up time |
|---|---|---|---|---|
| Upload processing | 40 | 3.9–7.6 s | 312 MB of 512 MB | 9 start-ups, 97–123 ms |
| API | 243 | 2 ms–12.6 s | 362 MB of 512 MB | 10 start-ups, 216–455 ms (includes downloading Cognito's keys) |
| Failure recorder | 1 | 0.9 s | 33 MB of 128 MB | 1 start-up, 119 ms |

- The processing figures are for the user's 60.6 MB export, every run, including failed attempts.
- The API's 12.6 s maximum is not attributed to a specific request; the likeliest candidate is
  building or serving the 63 MB processed export (inferred, not traced).
- All figures are well inside the limits set in the template (30 s, 512 MB), the API's worst case
  at about 42% of the time limit.

## Found during these runs

- **Intermittent processing failure** ("item not found" while saving reviews):
  [read-after-write analysis](2026-10-02-read-after-write-experiment.md). The retry and
  failure-recording path built for it was exercised live: retries shown on the page, then the
  upload marked failed after 3 attempts.
- **The sign-in shown on the page goes stale after an hour**, and requests then fail with 401.
- **A quiet restore of the last session can overtake a new load.**

- **The API logs nothing about each request.** Its log has only Lambda's own start/end/`REPORT`
  lines: no route, method or status. API Gateway access logging is not turned on in the template.
  So the logs can't say which request was a detection page, a flag save or an export; only the
  separate processing function's runs can be identified. Not yet in any plan.

Both later items are listed in the migration plan's follow-ups (see its hand-off section).
