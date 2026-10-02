# Development only: delete my earlier data before loading

**Status:** proposed 2026-10-02, not approved.

## Why

The user wants to test what happens on a *first* load of an export, repeatedly. On 2026-10-02 the
intermittent "item not found" failure turned out to happen only when review rows are newly
created (see [the analysis](../analysis/2026-10-02-read-after-write-experiment.md)); every
re-upload of the same file updates existing rows and can't show it. Getting back to a fresh state
took deleting rows by hand. The user asked for a checkbox next to the Load button, in development
versions only, that deletes earlier data before the load runs.

**"Development versions"** is taken to mean both the local server (`cargo run -p timeline-api`)
and a deployment whose stage is `dev`. Never a `staging` or `prod` deployment.

## Reuse check

- **Local server:** `POST /_dev/reset`
  ([dev_reset.rs](../../backend/timeline-api/src/routes/dev_reset.rs)) already empties every
  in-memory store; the browser tests use it. It empties *everyone's* data, which is fine locally
  but has no AWS counterpart: the deployed function never has the `_dev` routes, by design.
- **Stage check:** the template's `IsDev` condition
  ([template.yaml:65](../../infra/template.yaml#L65)) already switches dev-only behavior (the
  command-line password sign-in). Reused to switch this on.
- **Storage:** no port has a delete method today. Adding one to the existing traits would break
  the test doubles that implement them in committed tests (`tests/processing_errors.rs`,
  `tests/s3_trigger_progress.rs`, `tests/failed_upload.rs`), so a new, separate port is proposed.
- **Page:** the checkbox copies the existing "scan after uploading" checkbox's markup and style;
  the deployment settings file written by `scripts/write-deploy-config.sh` already tells the page
  which deployment it's using, and gains one field.

## Design

1. **A new port, `UserDataEraser`** (`timeline-core/src/ports/erase.rs`), one method:
   `erase_user_data(user_id) -> Result<ErasedCounts, EraseError>`, where `ErasedCounts` gives how
   many conversation summaries, upload records (outcome and progress rows), flag rows and stored
   files were deleted, and `EraseError` says what failed and the counts deleted before it, so a
   partial delete is never reported as none or as all.
2. **Two implementations:**
   - *In memory* (`timeline-storage/src/memory/eraser.rs`): removes only that user's entries from
     each in-memory store.
   - *AWS* (`timeline-storage/src/dynamo_s3_eraser.rs`):
     - conversations table: query `pk = <user>` (all of the user's `CONV#`, `UPLOAD#` and
       `PROGRESS#` rows) and delete each;
     - flags table: rows are keyed `<user>#<conversation>`, and a failed processing attempt can
       leave flag rows with no conversation summary, so they can't be found from the summaries.
       A **scan** with `begins_with(pk, "<user>#")` finds them all. A scan reads the whole table,
       which is acceptable for a development table and is one reason this stays development-only;
     - bucket: list and delete everything under `raw/<user>/` and `export/<user>/`.
     Deletes go one row at a time with the existing SDK calls; on any failure it stops and returns
     the error with the counts so far.
3. **A route, `DELETE /me/data`**, behind the normal login, deleting only the caller's data and
   answering `{"conversations": n, "uploads": n, "flags": n, "files": n}`. It exists only when the
   setting `TIMELINE_ALLOW_DATA_ERASE` is `on` (parsed once at start-up with the shared on/off
   helper in `aws_settings.rs`); otherwise the route isn't mounted and the answer is 404. The local
   server always mounts it. On a failure the answer is 500 with the error's message, including the
   partial counts.
4. **Template:** the API function's environment gets
   `TIMELINE_ALLOW_DATA_ERASE: !If [IsDev, "on", "off"]`, and a stack output `DataEraseEnabled`
   with the same value. The API function's permissions gain delete on the bucket if it lacks it
   (to be checked against its current policies when building).
5. **The page:** a checkbox directly beside the Load button: "Delete my earlier data first
   (development only)". Shown only when the page is using the local server, or the deployment
   settings file says `"allowDataErase": true` (written by `write-deploy-config.sh` from the
   `DataEraseEnabled` output). Unticked by default, and unticked again after each load. When
   ticked, Load first calls `DELETE /me/data`, shows "Deleting your earlier data…", then shows the
   counts deleted ("Deleted 118 conversations, 538 flags, 2 files") and continues with the
   upload. If the delete fails, the load stops and shows the server's message; nothing is
   uploaded.

## Post-addition check, applied to this design

- **Error paths:** every failure ends in a message on the page: the route's 500 carries the
  partial counts; a refused login is the usual 401 message; a missing route (setting off) is a
  404 the page shows as "deleting isn't available on this deployment" — it should never happen,
  since the checkbox is hidden then.
- **Stale display:** the checkbox's visibility comes from the deployment settings read at page
  load, which don't change while the page is open.

## Tests

- The port's contract, run against both implementations (in memory; DynamoDB Local plus the
  local S3 the tests already start): two users' data, one erased, the other untouched; flag rows
  with no conversation summary are erased too; counts are right; erasing a user with no data
  answers zeros.
- An AWS implementation failure (a missing table) returns an error with the counts so far.
- The route: deletes the caller's data and answers the counts; absent (404) with the setting off;
  refused without a login; the setting's parsing.
- Template: the setting is `on` only for the dev stage, the output matches, only the API
  function reads it.
- `write-deploy-config.sh`'s existing test script: the new field is written.
- Browser test (local server): load a file, tick the box, load again; the earlier review flags
  are gone and the counts are shown; with the delete answered by a 500, the load stops with the
  message and nothing is uploaded.

## Self-critique log

### C1 [RESOLVED]: Finding flag rows through the conversation summaries would miss some
A processing attempt that fails after saving reviews leaves flag rows with no summary (exactly
the state the user had to clean by hand on 2026-10-02). **Resolution:** the flags table is
scanned by key prefix; see [Design item 2 (line 40)](2026-10-02-dev-delete-before-load.md#L40).

### C2 [RESOLVED]: A production deployment must not be able to do this
**Resolution:** three independent gates: the template sets the setting `on` only when the stage
is `dev`; the route isn't mounted otherwise; the page shows the checkbox only when told it's
available. See [Design items 3–5 (line 53)](2026-10-02-dev-delete-before-load.md#L53).

### C3 [OPEN]: The load screen is to be rewritten after the deployment checks
This adds one control to a screen the user has called confusing. **Mitigation in plan:** the
checkbox sits directly beside the Load button, as asked. **Open:** trigger is the load-screen
rewrite, which should place it deliberately.
