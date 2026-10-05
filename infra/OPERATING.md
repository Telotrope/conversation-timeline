# Running and deploying, day to day

There are three ways to run the app. First-time setup (account, tools, certificates, teardown)
is in [README.md](README.md); this file is for everything repeated. The design is in
[docs/plans/2026-10-02-deployment-operating-guide.md](../docs/plans/2026-10-02-deployment-operating-guide.md).

| | Local | Remote development | Public |
|---|---|---|---|
| Purpose | Coding, browser tests | Testing on real AWS before release | Real users |
| Command | `scripts/dev-up.sh all` | `scripts/deploy.sh dev` | `scripts/deploy.sh public` |
| Page | `http://localhost:8000/timeline.html` (or `/proxy/8000/timeline.html` through VS Code's forwarding) | <https://dev.howangryami.telotrope.ai/> (and the local page, see below) | <https://howangryami.telotrope.ai/> |
| Backend | `timeline-api` on this machine, data in memory | Stack `timeline-dev` | Stack `timeline-public` |
| Sign-in | Dev login: a name, no password | Cognito; also password sign-in for scripts | Cognito |
| Records your activity | Yes, to the backend's output | Yes | No |
| Deploys from | The working tree | Any branch, uncommitted changes allowed | `main`, committed and pushed |

Each stack's settings are in [samconfig.toml](samconfig.toml), committed. AWS sign-in for the
remote ones: `aws login --profile timeline --remote` (lasts under a day). `aws sso` doesn't work
on this machine.

## Local

```
scripts/dev-up.sh all      # the page in the background, the backend in this terminal
scripts/dev-down.sh all    # stop both
```

`dev-up.sh` restarts the backend only when its code changed; restarting throws away its
in-memory uploads. `dev-up.sh backend` and `dev-up.sh static` start one half each.

- **Hard-reload after changing the page's code** (Ctrl+Shift+R). The local server sends no
  caching instructions, so the browser may keep old scripts. (The hosted pages send `no-cache`
  and don't need this.)
- **After a reload, wait for the page to settle before clicking Load.** It quietly restores the
  last session in the background, and a load started during that can be overtaken by it.
- **The local page against the remote development backend:** open
  `timeline.html?deploy=dev` once; `?deploy=` goes back to the local backend. The dev stack
  accepts the local page because its `AlsoAllowLocalPage` is on, for the address in its
  `FrontendUrl`.

## Remote development

```
scripts/deploy.sh dev
scripts/deploy.sh dev FailProcessing=on    # a setting changed for this deploy only
```

What it does, stopping with the step's name at any failure:

1. Checks AWS sign-in and that 3 GB of disk are free.
2. Builds the Lambdas and checks the template.
3. Makes a change set (AWS's list of what the deploy would change).
4. Checks it: anything **replaced or removed** (a replaced table or bucket loses its contents),
   and the API's CORS settings as AWS will actually apply them (on 2026-10-02 they were silently
   wrong; [analysis](../docs/analysis/2026-10-02-page-hosting-deployment.md)).
5. Applies it by itself when nothing is replaced or removed; otherwise lists what is and asks.
6. Publishes the page (`scripts/publish-page.sh dev`).
7. Checks the live site: the page, its settings file, CORS on the API and the upload bucket,
   and Cognito's return address. It asks CloudFront directly, so this works before DNS points
   at it.

If nothing changed it applies nothing but still publishes the page and checks the site. A
`Setting=value` lasts for that deploy only: the next plain `deploy.sh dev` puts
`samconfig.toml`'s value back.

**Reading the change list.** When a function's code changes, `HttpApi` and `RawUploadsBucket`
also show "Modify": they refer to the functions and AWS re-checks them. That's expected.
Replacements and removals are what `deploy.sh` stops for. `aws cloudformation
describe-change-set` gives AWS's reason for each change.

**Sign in afresh just before a test.** Cognito's sign-in lasts an hour, and an expired one shows
as a 401 even when processing succeeded.

### A test login from the command line (remote development only)

```
TOKEN=$(scripts/aws-dev-token.sh you@example.com)     # asks for your password
curl -H "Authorization: Bearer $TOKEN" "https://<ApiUrl>/conversations"
```

### Testing a first load: delete your review rows first

The intermittent "item not found" failure happens only when review rows are newly created, so
re-uploading the same file can't show it
([read-after-write analysis](../docs/analysis/2026-10-02-read-after-write-experiment.md)). Until
the planned delete checkbox exists
([plan](../docs/plans/2026-10-02-dev-delete-before-load.md)), on 2026-10-02 this was done by
hand:

1. Back up the table: `aws dynamodb scan --table-name timeline-message-flags-dev --output json > flags-backup.json`.
2. Delete each row with `aws dynamodb batch-write-item`, 25 delete requests per call (keys `pk`
   and `sk` from the backup), checking each call's `UnprocessedItems` is empty.
3. Confirm: `aws dynamodb scan --table-name timeline-message-flags-dev --consistent-read --select COUNT`
   shows 0.

### Watching an upload fail on purpose

AWS runs processing up to 3 times for one upload; if all fail, a second function marks it failed
and the page should say so
([plan](../docs/plans/2026-10-02-upload-processing-failures.md) §2–3).

1. `scripts/deploy.sh dev FailProcessing=on`
2. Upload a file. Expected, over about 4 minutes: "Waiting for the server to start", then
   "Processing on the server", then "The server hit an error (failing on purpose
   (FailProcessing is on)) on attempt 1 of 3…", attempts 2 and 3, then an error on the page.
3. `scripts/deploy.sh dev` puts it back.

### Capturing a real S3 notification for the tests

The tests' sample notification ([fixtures](../backend/timeline-api/tests/fixtures/aws-samples/))
was captured this way on 2026-10-02 (migration plan C24):

1. `scripts/deploy.sh dev LogS3Events=on`, then upload one small export.
2. `sam logs --stack-name timeline-dev -n ProcessUploadFunction --filter "s3 event"` shows a line
   starting `s3 event (sourceIPAddress removed):`.
3. `scripts/deploy.sh dev` switches it off again.
4. Replace the account number, bucket name, user ID and AWS's internal IDs with placeholders and
   review it before it replaces the sample.

### Checks after a big backend change

The first deployment's checks (migration plan §V2e). `deploy.sh` covers D1 and D2 itself; the
others need a person:

| # | What to check | How |
|---|---|---|
| D1 | AWS accepted the template | `deploy.sh` finished |
| D2 | CORS allows only the page | `deploy.sh`'s live checks; also `curl -i -X OPTIONS "$API/conversations" -H "Origin: http://example.com" -H "Access-Control-Request-Method: GET"` has no `access-control-allow-origin` |
| D3 | Real Cognito sign-in works | sign in on the page; `scripts/aws-dev-token.sh` |
| D4 | The API refuses bad logins | `curl -i "$API/conversations"`, and with `-H "Authorization: Bearer nonsense"`: both 401 |
| D5 | A ~60 MB export is processed within the limits | upload it; each call's `REPORT` line in `sam logs --stack-name timeline-dev -n ProcessUploadFunction` (and `-n ApiFunction`) shows `Duration` and `Max Memory Used` |
| D6 | The flag-handle secret reached the API | tick a flag in the Review tab: the page says it saved |
| D7 | The tables match what the tests assume | the whole page flow works |
| D9 | Start-up time | a function's first `REPORT` line shows `Init Duration` |

## Public

```
scripts/deploy.sh public
```

The same steps as remote development, with two differences: it refuses unless you're on `main`
with nothing uncommitted or unpushed (so the page's recorded version names a real commit), and
it always shows the change list and asks `Apply to timeline-public? [y/N]` once.

Release by deploying to remote development first, checking the change there, then pushing
`main` and deploying public.

## When something goes wrong

- **One upload:** `scripts/diagnose-upload.sh <dev|public> <upload-id>` shows when the file
  landed in S3, what DynamoDB holds for it (outcome, attempt count, last error) and its
  requests and processing attempts from the logs.
- **Everything recently:** `scripts/activity-timeline.sh <dev|public> --since 2h` (add
  `--session <id>` for one page session, `--upload <id>` for one upload).
- **A deploy stopped:** its last `==` line names the step. For "ended in UPDATE_ROLLBACK_…",
  the stack's events in the CloudFormation console give AWS's reason.

## The build folder

It cleans itself: after any Claude Code turn, a background check runs
`scripts/clean-build.py` once `backend/target/debug` passes 12 GB, deleting outdated copies and
keeping everything current ([plan](../docs/plans/completed/2026-10-02-automatic-build-cleanup.md)). Its
log is `backend/target/clean-build.log`. `deploy.sh` also stops if less than 3 GB is free.
