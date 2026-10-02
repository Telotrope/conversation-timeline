# Deployment checks: status on 2026-10-02

The checks only a real deployment can do are defined in the migration plan's table
([migration plan, line 1391](../plans/2026-09-09-rust-aws-backend-migration.md#L1391)). This records
where each stands on the `dev` stack, as of its last deploy (2026-10-02 16:47 UTC). Nothing here was
run as a deliberate check unless the table says so; most evidence comes from the user's own uploads
through the page, read afterwards from the functions' logs.

**Headline: one check passed (D1), one passed in part (D3), two have partial measurements (D5, D9),
four have not been run (D2, D4, D6, D7).**

## Status of each check

| Check | What it verifies | Status | Evidence |
|---|---|---|---|
| D1 | AWS accepts the template | **Passed** | `sam deploy` succeeded, including the 2026-10-02 deploy adding `RecordFailedUploadFunction` (the user confirmed the change list after Claude checked AWS's reason for each change) |
| D2 | The browser's pre-flight `OPTIONS` request answered without login, from the allowed address only | **Not run** | The page's requests work from the user's address, which shows the allowed address is accepted; the refusal of other addresses has not been tried |
| D3 | Real Cognito login: hosted page, code exchange, command-line script | **Page part passed; script part not run** | The user signs in through Cognito's page and uploads; `scripts/aws-dev-token.sh` has not been run |
| D4 | The API refuses no token, a bad token, another pool's token | **Not run** | One incidental observation: a request with an expired token was refused with 401 by API Gateway (17:53 UTC) |
| D5 | ~60 MB export processed within memory and time limits; detection pass pages each under 30 s | **Upload part measured; detection not run** | See the measurements below. No upload with the detection checkbox ticked has run on AWS, so the 30-second limit for detection pages is untested |
| D6 | The flag-handle secret reaches the API | **Not run** | No flag save from the page on AWS has been checked |
| D7 | Real tables' keys match the tests' assumptions | **Not run as a check** | Uploads complete and the timeline displays with its review flags, which exercises the conversations and flags tables' keys; the DynamoDB console comparison has not been done |
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
