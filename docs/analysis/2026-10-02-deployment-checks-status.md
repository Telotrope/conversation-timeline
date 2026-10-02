# Deployment checks: status on 2026-10-02

The checks only a real deployment can do are defined in the migration plan's table
([migration plan, line 1391](../plans/2026-09-09-rust-aws-backend-migration.md#L1391)). This records
where each stands on the `dev` stack, as of its last deploy (2026-10-02 16:47 UTC). Nothing here was
run as a deliberate check unless the table says so; most evidence comes from the user's own uploads
through the page, read afterwards from the functions' logs.

**Headline (updated 18:54 UTC): three checks passed (D1, D2, D7), one passed in part (D4), one passed in part and failed in part (D3: the command-line script is broken), two
have partial measurements (D5, D9), one has not been run (D6).**

## Status of each check

| Check | What it verifies | Status | Evidence |
|---|---|---|---|
| D1 | AWS accepts the template | **Passed** | `sam deploy` succeeded, including the 2026-10-02 deploy adding `RecordFailedUploadFunction` (the user confirmed the change list after Claude checked AWS's reason for each change) |
| D2 | The browser's pre-flight `OPTIONS` request answered without login, from the allowed address only | **Passed** (Claude, 18:54 UTC) | `curl -X OPTIONS .../conversations` with `Origin: https://dev.tail13dce8.ts.net`: 204 with `access-control-allow-origin: https://dev.tail13dce8.ts.net`, allowed methods `GET,PATCH,POST`, headers `authorization,content-type`. With `Origin: http://example.com` and `http://localhost:8000`: 204 with no `access-control-*` headers, so a browser blocks them. The page's own requests work (earlier uploads) |
| D3 | Real Cognito login: hosted page, code exchange, command-line script | **Page part passed; script part FAILED** (user ran it, ~19:00 UTC) | The user signs in through Cognito's page and uploads. `scripts/aws-dev-token.sh` failed before reaching Cognito: `ParamValidation: Error parsing parameter 'cli-input-json': Invalid JSON received`. Cause, found by Claude with a made-up client ID (no login attempted): aws-cli 2.37.8 rejects valid JSON piped through `file:///dev/stdin` ([aws-dev-token.sh:25](../../scripts/aws-dev-token.sh#L25)), but accepts the same JSON from an ordinary file (answering "client does not exist"). The script's test uses a stand-in `aws` that never parses the JSON, so it couldn't catch this. Not fixed |
| D4 | The API refuses no token, a bad token, another pool's token | **Two of three parts passed; other pool's token not run** (Claude, 18:54 UTC) | `GET /conversations` with no `Authorization` header, and with `Bearer nonsense`: both 401 `{"message":"Unauthorized"}`. Earlier, an expired token was also refused with 401 (17:53 UTC). No validly signed token from another issuer has been tried |
| D5 | ~60 MB export processed within memory and time limits; detection pass pages each under 30 s | **Upload part measured; detection not run** | See the measurements below. No upload with the detection checkbox ticked has run on AWS, so the 30-second limit for detection pages is untested |
| D6 | The flag-handle secret reaches the API | **Not run** | No flag save from the page on AWS has been checked |
| D7 | Real tables' keys match the tests' assumptions | **Passed** (Claude, 18:54 UTC) | `aws dynamodb describe-table`: `timeline-conversations-dev` and `timeline-message-flags-dev` both have `pk` (string, HASH) + `sk` (string, RANGE), no secondary indexes, the same layout the test helper creates ([dynamodb_local.rs:166](../../backend/timeline-storage/tests/support/dynamodb_local.rs#L166)). `timeline-users-dev` has `pk` only; no Rust code reads it (a search for its environment variable `TIMELINE_USERS_TABLE` finds only the template), so the tests have nothing to match there. Uploads and review flags also work against the real tables |
| D9 | Start-up time, including downloading Cognito's keys | **Measured; no pass/fail threshold set** | See below |

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

Both later items are listed in the migration plan's follow-ups (see its hand-off section).
