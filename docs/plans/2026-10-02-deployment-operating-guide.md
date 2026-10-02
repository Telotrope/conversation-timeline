# Repeat-deployment guide and two helper scripts

**Status:** outline only, written 2026-10-02 at the user's request. **Not approved and not ready to
build.** A fresh session should rewrite this into a full plan (design, reuse check, tests,
self-critique log per CLAUDE.md), then ask the user to approve it.

## Why

[infra/README.md](../../infra/README.md) is a first-time setup walkthrough, but it has grown steps
that are done on every deployment, and several practical lessons from the 2026-10-02 deployment
session were never written anywhere. The user proposed separating first-time setup from repeated
deployments. Status (what is deployed, which checks passed) belongs in `docs/analysis/`, e.g.
[2026-10-02-deployment-checks-status.md](../analysis/2026-10-02-deployment-checks-status.md), not
in either guide.

## What is needed

1. **Keep [infra/README.md](../../infra/README.md) for first-time setup only:** account, tools,
   first deploy, first sign-up, teardown.
2. **A new `infra/OPERATING.md` for repeated work:** redeploying, testing an upload, diagnosing a
   failed one. Walkthrough steps 8–10 (the deployment checks, capturing a real S3 notification,
   the fail-on-purpose test) move there.
3. **Two scripts**, so repeated steps are run rather than retyped, and break loudly when wrong:
   - **Redeploy:** check free disk space, build the Lambdas, check the template, deploy with the
     saved settings, stop before confirming so the change list can be read.
   - **Diagnose an upload:** given an upload's ID, print its timeline: when the file landed, each
     processing attempt and its error, the stored outcome and attempt count. Claude did this by
     hand on 2026-10-02 with `aws s3 ls`, `aws logs filter-log-events` and `aws dynamodb get-item`.

## Lessons to write into `infra/OPERATING.md` (from the 2026-10-02 session, not yet written anywhere)

1. **Which AWS login command is right is unresolved.** README step 3 says `aws sso login`; the
   2026-10-02 session used `aws login --profile timeline --remote`, which lasts under a day. Ask
   the user which they use. (The Rust AWS SDK needs `aws-config`'s `credentials-login` feature to
   use `aws login`; already enabled for the tests.)
2. **Reading the change list before confirming a deploy.** When a function's code changes,
   `HttpApi` and `RawUploadsBucket` also show "Modify": they refer to the functions' identifiers
   and AWS re-checks them. That's expected. Anything marked replaced (`Replacement: True`) or
   removed needs a closer look. `aws cloudformation describe-change-set` gives AWS's reason for
   each change.
3. **Hard-reload the page after code changes** (Ctrl+Shift+R). The local `python3 -m
   http.server` sends no caching instructions, so the browser kept old scripts on 2026-10-02 and
   the new progress display didn't appear.
4. **After reloading, wait for the page to settle before clicking Load.** The page quietly restores
   the last session in the background, and a new load started during it was overtaken by the old
   data. (A restore screen with a Stop button is on the migration plan's follow-up list.)
5. **Testing a first load needs your review rows deleted first.** The intermittent "item not
   found" failure happens only when review rows are newly created, so re-uploading the same file
   can't show it ([read-after-write analysis](../analysis/2026-10-02-read-after-write-experiment.md)).
   The deletion on 2026-10-02 was done in another session and how it was done isn't recorded; ask
   the user. The planned delete checkbox
   ([2026-10-02-dev-delete-before-load.md](2026-10-02-dev-delete-before-load.md)) would replace this.
6. **Diagnosing a failed upload:** what the diagnosis script automates (above). Also: sign in
   afresh just before a test, since Cognito's sign-in lasts an hour and an expired one makes the
   page report a 401 even when processing succeeds; and the Rust build folder grows to about 30 GB
   and filled the disk on 2026-10-02 (`cargo clean` in `backend/` frees it).

## Hosting the page (moved here from the page-hosting plan's §4, 2026-10-02)

The user moved this from [2026-10-02-page-hosting.md](2026-10-02-page-hosting.md) so all deployment
documentation is written together, after that plan has been tested on a real stack.

- **First-time setup (README):** deploying with `HostPage=on`, optionally `PageDomain` (e.g.
  `howangryami.telotrope.ai`) and `AlsoAllowLocalPage=on` for a dev stack; the two `CNAME`
  records added by hand at Porkbun (certificate validation while the deploy waits, then the
  domain → the `PageDnsTarget` output); running `scripts/publish-page.sh <stage>` the first time.
- **Repeated work (`OPERATING.md`):** run `scripts/publish-page.sh <stage>` after every page
  change, and after every redeploy that changes the stack's outputs (the settings file is
  rebuilt from them). Consider whether the redeploy script should run it.
- **The migration plan's C30:** re-tag it `[OPEN, cross-plan]` pointing at the page-hosting plan;
  `[RESOLVED]` once that plan's deployment checks H1–H10 pass.

## Directions for the rewrite

- Run the reuse check: existing scripts in [scripts/](../../scripts/) (`check-template.sh`,
  `write-deploy-config.sh`, `aws-dev-token.sh`) and how they're tested; the scripts should follow
  their style and testing.
- Decide how the scripts are tested without touching AWS, and which parts can only be checked live.
- Decide whether `samconfig.toml` (local, ignored by git) is the source of the saved settings the
  redeploy script uses, and what it does when the file is missing.
- Moving steps 8–10 out of the README changes links elsewhere (the migration plan refers to "step
  9"); find and update them.
