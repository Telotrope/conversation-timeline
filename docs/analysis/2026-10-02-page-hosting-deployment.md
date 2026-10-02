# Page hosting: first deployment (dev stack), 2026-10-02

Deployment of [the page hosting plan](../plans/2026-10-02-page-hosting.md) to stack `timeline-dev`
at commit `1cc52de`, with `HostPage=on`, `PageDomain=howangryami.telotrope.ai`,
`PageCertificateArn` = the certificate the user made by hand, and `AlsoAllowLocalPage=on`.
These settings are saved in the git-ignored `infra/samconfig.toml`, so a plain `sam deploy` keeps hosting on.

The deploy also released the activity-logging work committed earlier that day by another session
(`ActivityFunction`, five log groups, API request log), which the stack didn't have yet. The
change set replaced and removed nothing.

## Before deploying: build folder cleanup

The disk had 639 MB free; `backend/target` was 30 GB, mostly outdated copies (the open item C3 in
[the smaller-debug-builds plan](../plans/completed/2026-10-01-smaller-debug-builds.md)). Cargo
listed every file its current builds use (`cargo test --workspace --no-run`, `cargo build
--workspace --bins --examples`, `cargo build -p timeline-api`, with `--message-format=json`), and
everything else in `target/debug/{deps,.fingerprint,build}` was deleted, plus older incremental
folders beyond each crate's count of current builds. Result: 18 GB free, `target` 14 GB. A repeat
of the listing found 1,173 artifacts current and 10 relinked (the binaries and examples, whose
files carry no hash, so the filter removed them). No library was rebuilt.

## Deployment checks

| # | Check | Result |
|---|---|---|
| H1 | Page loads; bucket refuses direct access | **Pass**: `https://d3dl4z1yvtflex.cloudfront.net/` 200; the bucket's S3 address 403 |
| H2 | `.js` served as JavaScript | **Pass** for `frontend/main.js` (`text/javascript; charset=utf-8`). Modules running in a browser not yet observed |
| H3 | Cognito sign-in returns to the page | **Not run**: needs a browser sign-in |
| H4 | Loading, uploading, flag edits pass CORS from the hosted origin | **Not run** |
| H5 | A second publish shows the new version | **Not run** |
| H6 | Compression and security headers | **Pass**: `content-encoding: br`; HSTS, nosniff, frame and referrer headers present |
| H7 | A week's cost at $0.00 | **Not run** (due 2026-10-09) |
| H8 | Custom domain serves with the given certificate | **Partial pass**: requests for `howangryami.telotrope.ai` sent to CloudFront's address return 200 with a valid certificate; Certificate Manager shows it in use and renewal `ELIGIBLE`. The Porkbun `CNAME` (`howangryami` → `d3dl4z1yvtflex.cloudfront.net`) did not exist yet |
| H9 | Sign-in from `/`; files load below `/` | **Not run** |
| H10 | The local page still works against the hosted stack | **Not run** |

Also observed: the published `index.html` carries `<meta name="timeline-deploy" content="dev">`,
and `frontend/deploy-configs/dev.json` is served with the stack's API, Cognito and client values
and `pageVersion` `1cc52de`.
