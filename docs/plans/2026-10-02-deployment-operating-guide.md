# Deploying three ways: local, remote development, public

**Status:** draft, awaiting approval. Rewrites the outline of the same name (2026-10-02) at your
request: explicit, automated steps for three kinds of deployment.

## Why

Today one stack, `timeline-dev`, does two jobs. It is the development stack (password sign-in for
scripts, activity recording, `FailProcessing` tests), and since 2026-10-02 it also serves the
public address `howangryami.telotrope.ai`. Deploying it took about twenty hand-typed commands
and two fixes found only after deploying
([analysis](../analysis/2026-10-02-page-hosting-deployment.md)). You want three separate ways to
run the app, each one command where possible:

| | Local | Remote development | Public |
|---|---|---|---|
| Purpose | Coding, browser tests | Testing on real AWS before release | Real users, who will pay |
| Backend | `timeline-api` on this machine, in memory | Stack `timeline-dev` | New stack `timeline-public` |
| Page address | `localhost:8000` or your Tailscale address | `https://dev.howangryami.telotrope.ai/` (also your Tailscale page) | `https://howangryami.telotrope.ai/` |
| Sign-in | Dev login (a name, no password) | Cognito, plus password sign-in for scripts | Cognito only |
| Activity recording | On (to the local log) | On | Off (activity plan §9: dev only) |
| Deployed from | Working tree | Any branch, any state | `main` only, committed, no local changes |
| Command | `scripts/dev-up.sh all` | `scripts/deploy.sh dev` | `scripts/deploy.sh public` |

"A remote development branch" means a separate **deployment** (a stack) for testing, deployable
from any git branch, not a git branch of its own (confirmed 2026-10-02).

## Reuse check

- [scripts/dev-up.sh](../../scripts/dev-up.sh) already starts the local backend or the static
  page, restarting only what's out of date. It gains an `all` mode (§4); not rewritten.
- [scripts/check-template.sh](../../scripts/check-template.sh),
  [scripts/write-deploy-config.sh](../../scripts/write-deploy-config.sh) and
  [scripts/publish-page.sh](../../scripts/publish-page.sh) are called by `deploy.sh` as they are.
- SAM's own **config environments** (`sam deploy --config-env <name>`, sections
  `[dev.deploy.parameters]` and `[public.deploy.parameters]` in `samconfig.toml`) hold each
  stack's settings. No settings format of our own.
- The checks I ran by hand on 2026-10-02 (`curl` for the page, the settings file and CORS
  preflights; reading the processed template) become `deploy.sh`'s checks.
- [scripts/test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh)'s stand-in `aws` command
  is extended for the new scripts' tests, not duplicated.

## Design

### §1. Settings for both stacks, in git

[infra/samconfig.toml](../../infra/samconfig.toml) becomes a committed file (removed from
[.gitignore](../../.gitignore)) with two environments:

- `dev`: stack `timeline-dev`, `Stage=dev`, `HostPage=on`,
  `PageDomain=dev.howangryami.telotrope.ai` with its certificate, `AlsoAllowLocalPage=on`,
  `FrontendUrl` = your Tailscale page address.
- `public`: stack `timeline-public`, `Stage=public`, `HostPage=on`,
  `PageDomain=howangryami.telotrope.ai` with the existing certificate, `AlsoAllowLocalPage=off`.

Why commit it: today it's ignored as "one person's answers". But the one person is you, and a
deploy that depends on an uncommitted file can't be repeated or reviewed. On 2026-10-02 a plain
`sam deploy` from the old file would have deleted the CloudFront distribution. Nothing in it is
secret: certificate identifiers and the account number are visible to anyone the page talks to.

### §2. `scripts/deploy.sh <dev|public>`

One command per remote deploy. Every step stops the script with a message naming the failed step.

1. **Before anything:**
   - checks that AWS sign-in is valid (`aws sts get-caller-identity`, profile `timeline`), and
     when it has expired says to run `aws login --profile timeline --remote`. `aws sso` doesn't
     work on this machine;
   - checks there are at least 3 GB free on the disk;
   - for `public` only, refuses unless on `main` with no local changes and nothing unpushed. The
     page's recorded version (`git describe --dirty`) then names a real commit.
2. **Build:** `cargo lambda build --release --arm64 -p timeline-api`, then
   [check-template.sh](../../scripts/check-template.sh).
3. **Change set:** `sam deploy --config-env <env> --no-execute-changeset`.
4. **Checks on the change set**, each a failure with its reason:
   - lists anything replaced or removed;
   - reads AWS's **processed** template for the change set
     (`aws cloudformation get-template --template-stage Processed`). Every
     `x-amazon-apigateway-cors` must be a CORS object with `allowOrigins`, or a condition whose
     branches all are. This catches the 2026-10-02 failure, where SAM turned it into a bare list.
5. **Apply:**
   - `dev`: applies by itself when nothing is replaced or removed. Otherwise it prints the change
     list and asks.
   - `public`: always prints the change list and asks `Apply to the public site? [y/N]`. This is
     the one question in the whole flow.
   - Then it waits for the stack to finish.
6. **Publish the page:** [publish-page.sh](../../scripts/publish-page.sh) `<env>` (unchanged, and
   it writes the settings file itself).
7. **Checks on the live site** (the ones done by hand on 2026-10-02):
   - the page loads from `PageUrl` with its tag naming this deployment;
   - the settings file is served and names this stack's API;
   - CORS check requests from the page's address are accepted by the API (`POST /uploads`) and
     the upload bucket (`PUT`);
   - Cognito's return addresses include `PageUrl`.
8. Prints `PageUrl` and, for a new custom domain, the `CNAME` to add at Porkbun.

### §3. `scripts/request-certificate.sh <domain>`

For a new custom domain, done once per name:

1. Requests a public certificate in us-east-1.
2. Prints the validation `CNAME` to add at Porkbun.
3. Waits until AWS shows it issued.
4. Prints the identifier to put in `samconfig.toml`.

Adding the record at Porkbun stays by hand. Automating it means storing a Porkbun API key; not
worth it for a step done once per domain.

### §4. Local: `scripts/dev-up.sh all`

A new mode that runs the existing `backend` and `static` modes together. It prints the page
address both locally and through your Tailscale forwarding (`/proxy/8000/timeline.html`). Nothing
else changes about local development.

### §5. `scripts/diagnose-upload.sh <env> <upload-id>`

From the outline. It prints one upload's timeline:
- when the file landed in S3;
- each processing attempt and its error, from the processing logs;
- the stored status and attempt count from DynamoDB.

I did this by hand on 2026-10-02 with `aws s3 ls`, `aws logs filter-log-events` and
`aws dynamodb get-item`. It reuses
[scripts/activity-timeline.sh](../../scripts/activity-timeline.sh)'s log reading where that fits;
which parts fit is decided during implementation and reported.

### §6. Documentation

- [infra/README.md](../../infra/README.md): first-time setup only. That covers the account, tools,
  first sign-up and teardown, plus, once per domain, `request-certificate.sh` and the Porkbun
  records.
- New `infra/OPERATING.md`: the three ways (the table above, then each one's command and what to
  check afterwards), and the README's current steps 8–10 moved in. Also the lessons below. Links
  to "step 9" elsewhere (the migration plan) are updated.
- The migration plan's C30 is re-tagged `[RESOLVED]`, pointing at the page-hosting plan: its
  deployment checks passed on 2026-10-02 except H5 and H7.

Lessons to write into `OPERATING.md` (from 2026-10-02):

1. Read the change list before confirming. Function code changes make `HttpApi` and
   `RawUploadsBucket` show "Modify", which is expected. Replacements and removals are what to
   look at. `deploy.sh` now flags these.
2. Hard-reload the local page after code changes (the local server sends no caching
   instructions). The hosted page needs no hard reload: it sends `no-cache`.
3. After reloading, wait for the page to settle before clicking Load. The background restore can
   overtake a new load.
4. Testing a first load needs your review rows deleted first
   ([read-after-write analysis](../analysis/2026-10-02-read-after-write-experiment.md)). On
   2026-10-02 this was done with a one-off script: back up the `timeline-message-flags-dev`
   table, then `aws dynamodb batch-write-item` delete requests 25 at a time, then a consistent
   `scan --select COUNT` to confirm zero. The planned delete checkbox
   ([2026-10-02-dev-delete-before-load.md](2026-10-02-dev-delete-before-load.md), not yet built)
   replaces this; until then `OPERATING.md` records those steps.
5. Sign in afresh just before a test: Cognito's sign-in lasts an hour, and an expired one shows
   as a 401 even when processing succeeded.
6. The build folder is cleaned automatically
   ([automatic build cleanup plan](2026-10-02-automatic-build-cleanup.md)), and `deploy.sh`
   checks free space.

### §7. Moving the public site to its own stack (once, at the first `deploy.sh public`)

1. `request-certificate.sh dev.howangryami.telotrope.ai`, and add its validation record at
   Porkbun.
2. `deploy.sh dev`, now with `dev.howangryami.telotrope.ai`. This releases
   `howangryami.telotrope.ai` from the dev stack's CloudFront distribution: a name can be on
   only one distribution. **The public address is down from here until step 4.**
3. At Porkbun: `CNAME` `dev.howangryami` → the dev stack's `PageDnsTarget`.
4. `deploy.sh public`. This creates the new stack, then you point the Porkbun `CNAME`
   `howangryami` at the public stack's `PageDnsTarget`.

The public stack starts empty: new sign-in accounts (sign up again) and no uploads. Your data on
`timeline-dev` stays there (open question 3).

## Tests

All run on this machine without AWS, using stand-ins for `aws`, `sam`, `cargo` and `curl` in
[scripts/test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh)'s style.

- **`deploy.sh`:**
  - `public` refuses off `main`, with local changes, or with unpushed commits; `dev` doesn't;
  - stops with a message on low disk, an expired sign-in, a failed build or template check;
  - the processed-template CORS check rejects the shape from 2026-10-02 and accepts the fixed
    one. Both are taken from the real processed templates AWS returned that day, trimmed to the
    `HttpApi` resource and saved as test fixtures, so the check is tested against what AWS
    actually produces;
  - `dev` applies by itself with no replacements and asks when there are some;
  - `public` always asks, and `n` applies nothing;
  - each live-site check, failing, stops the script and names the check;
  - the commands run in order: build, check, change set, checks, apply, publish, live checks.
- **`request-certificate.sh`:** prints the validation record from Certificate Manager's
  documented output, waits for "issued", and prints the identifier.
- **`diagnose-upload.sh`:** builds the timeline from canned S3, log and DynamoDB answers in the
  CLI's documented shapes, and names a missing upload.
- **`dev-up.sh all`:** a case in [scripts/test-dev-up.sh](../../scripts/test-dev-up.sh) that both
  halves start.

**Only real deployments can check the rest.** §7 run end to end, then:
- `deploy.sh dev` twice, the second time with nothing changed (it should apply nothing);
- one page change through `deploy.sh dev`, then `deploy.sh public`.

Results go to a `docs/analysis/` file. Until then, the status is "code-level only, end-to-end
TBD".

## Answered 2026-10-02

1. AWS sign-in is `aws login --profile timeline --remote`; `aws sso` doesn't work here (§2 step 1).
2. Review rows were deleted with a one-off script (lesson 4).

## Open question for you

1. Is it acceptable that the public stack starts with no accounts and no data (§7)?

## Self-critique log

### C1 [RESOLVED]: One stack served both development and the public
Original concern: password sign-in, activity recording and failure-injection settings are dev
features. They'd be live on the public address. **Resolution:** a separate `timeline-public`
stack with `Stage=public`, where those conditions are off
([§1 (line 45)](2026-10-02-deployment-operating-guide.md#L45), [§7 (line 154)](2026-10-02-deployment-operating-guide.md#L154)).

### C2 [RESOLVED]: An uncommitted settings file decided what a deploy did
Original concern: `samconfig.toml` was git-ignored, and a stale copy would have turned hosting
off. **Resolution:** committed, with one environment per stack
([§1 (line 45)](2026-10-02-deployment-operating-guide.md#L45)).

### C3 [RESOLVED]: Deploy problems were found only after deploying
Original concern: on 2026-10-02 the API's CORS was broken by SAM's conversion, and was found
only when your load failed. **Resolution:** `deploy.sh` checks the processed template before
applying, and the live site's CORS after
([§2 (line 61)](2026-10-02-deployment-operating-guide.md#L61)).

### C4 [OPEN]: The public address is down during the one-time move
Between §7 steps 2 and 4, `howangryami.telotrope.ai` points at nothing that answers for it.
**Mitigation in plan:** steps 2–4 are minutes apart, and there are no users yet. **Open:** none
unless users exist before the move. Trigger: someone other than you uses the site before §7 runs.

### C5 [OPEN]: Remote dev deploys apply without asking
`deploy.sh dev` applies when nothing is replaced or removed, so a mistaken but harmless-looking
change goes live on dev unseen. **Mitigation in plan:** the processed-template and live-site
checks; dev holds test data only. **Open:** make dev ask too. Trigger: a dev deploy you didn't
expect to change something did.
