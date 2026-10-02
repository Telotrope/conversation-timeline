# Hosting the page (closing the migration plan's C30)

**Status:** draft, awaiting review. No code written.

## Why

The backend runs on AWS, but `timeline.html` is still served from the developer's machine
([migration plan, C30](2026-09-09-rust-aws-backend-migration.md#L2124)). Nobody else can use the
app until the page lives somewhere public. This plan compares places to put it, recommends one,
and describes the work.

Terms used below:

- **Static hosting**: serving files exactly as stored, with no program running per request. The
  page needs nothing more: it is 37 files (`timeline.html`, 35 JavaScript files under
  [frontend/](../../frontend/), one library in [vendor/](../../vendor/)), 245 KB in total, about
  83 KB compressed (measured with `gzip` on 2026-10-02). There is no build step.
- **CDN** (content delivery network): a provider's servers around the world that keep copies of
  the files and serve each visitor from a nearby one.
- **Origin**: a web address's scheme and host, e.g. `https://telotrope.github.io`. Browsers use it
  to decide which sites may call which APIs.
- **CORS** (cross-origin resource sharing): the rule by which our API and upload bucket accept
  calls only from the page's origin.
- **SLA** (service level agreement): the provider's written uptime promise. Breaking it earns a
  refund credit, not compensation for lost business. An SLA is a promise, not a measurement.

## What any host must do for this page

1. **Its address must be wired into three places in the stack**, all currently derived from the
   `FrontendUrl` parameter ([infra/template.yaml (line 30)](../../infra/template.yaml#L30)):
   Cognito's allowed return addresses
   ([line 229](../../infra/template.yaml#L229)), the upload bucket's CORS
   ([line 146](../../infra/template.yaml#L146)) and the API's CORS
   ([line 442](../../infra/template.yaml#L442)). Any address not listed there cannot sign in or
   call the API. This also means a host's per-branch "preview" addresses are useless to us
   unless each is added to the stack.
2. **It must serve the deployment's settings file**, which git ignores
   ([.gitignore](../../.gitignore)) and [scripts/write-deploy-config.sh](../../scripts/write-deploy-config.sh)
   writes from the stack's outputs. So publishing has to run after a deploy, with AWS access.
3. **It must serve `.js` files as JavaScript.** Browsers refuse to run a module script sent with
   any other content type.

## Comparison

### Traffic assumptions

One first visit downloads 37 files plus the settings file: 38 requests, about 83 KB. A repeat
visit still sends 38 requests (the browser asks "has this changed?" for each file, because file
names don't change between versions; see C4) but downloads little. So **request counts, not
bandwidth, are what hit free-tier limits first.** Three levels:

| Level | Page loads / month | Requests | Data |
|---|---|---|---|
| Testing | 1,000 | 38,000 | 0.08 GB |
| Small launch | 20,000 | 760,000 | 1.7 GB |
| Growth | 200,000 | 7.6 million | 17 GB |

### Monthly cost (prices checked 2026-10-02; sources at the end)

| Host | Testing | Small launch | Growth | Notes |
|---|---|---|---|---|
| GitHub Pages | $0 | $0 | $0 | Free only if the repository is public; a private one needs a paid GitHub plan. **Terms forbid SaaS (software sold as a service).** |
| AWS S3 + CloudFront, pay-as-you-go | ≈$0 | ≈$0 | ≈$0 | CloudFront's permanent free allowance is 1 TB and 10 million requests a month; beyond it, $0.085/GB in North America and Europe. S3 storage for 245 KB is a fraction of a cent. |
| AWS S3 + CloudFront, flat-rate "Free" plan | $0 | $0 | **$15** (Pro plan) | Free plan: 1 million requests, 100 GB, no overage charges. Growth exceeds 1 million requests. What happens past the allowance isn't stated on the pricing page. Not available on AWS "Free Tier" accounts. |
| AWS Amplify Hosting | $0 | $0 | ≈$0.30 | 15 GB/month free, then $0.15/GB. I did not confirm whether that free allowance is permanent or only for a new account's first year. |
| Cloudflare Pages | $0 | $0 | $0 | Requests for static files are unmetered on the free plan. |
| Netlify | $0 | $0 (≈200 of 300 free credits) | **$20** (Pro) | Credits: 2 per 10,000 requests, 20 per GB, **15 per deploy**. On the free plan, running out pauses every site on the account until the month resets. |

A custom domain adds nothing at any of these hosts except AWS, where Route 53 (AWS's DNS service,
which maps names to servers) costs $0.50/month per domain *if* the domain's DNS is moved there.
It's included in the flat-rate plans and not needed if the DNS stays where it is now.

### Uptime

| Host | SLA |
|---|---|
| GitHub Pages | None on free or Team plans. Enterprise Cloud: 99.9% per quarter. I found that in GitHub's *deprecated* SLA page; current terms not checked. |
| CloudFront | 99.9% per month |
| Amplify Hosting | 99.95% per month, per region |
| Cloudflare | None on the free plan; 100% on Business ($200/month) |
| Netlify | None below Enterprise |

I have no measured uptime for any of these. Netlify's forum cites 99.98% measured by Pingdom, but
that is the provider's own claim. **The more important point is that the page is useless without
the API.** Login, loading and saving all go through Cognito, API Gateway and Lambda in our AWS
region. So hosting the page somewhere else buys no independence: if our AWS region is down, the
app is down wherever the page is. A non-AWS host adds a second way to fail. It doesn't remove one.

### Other factors

| Factor | GitHub Pages | S3 + CloudFront | Amplify | Cloudflare Pages | Netlify |
|---|---|---|---|---|---|
| Allowed for a paid product (V4 adds payment) | **No** | Yes | Yes | Yes | Yes |
| Own origin without a custom domain | **No**: `telotrope.github.io` is shared by every Pages site in the organization | Yes (`xxxx.cloudfront.net`) | Yes | Yes (`name.pages.dev`) | Yes (`name.netlify.app`) |
| Can set security headers (e.g. HSTS, which tells browsers "HTTPS only") | No | Yes, by policy in the template | Yes | Yes, via a `_headers` file | Yes, via a `_headers` file |
| Part of the same SAM template, so one deploy wires its address into Cognito and CORS | No | **Yes** | Yes | No | No |
| New vendor account holding a deploy credential | No (GitHub already) | No | No | Yes | Yes |
| Publishing needs AWS access anyway (settings file) | Yes, from CI (continuous integration, GitHub's automated job runner) | Yes, same machine as `sam deploy` | Yes | Yes, from CI | Yes, from CI |
| Bill risk under a traffic flood | None | Pay-as-you-go: unbounded, warned by the $10 budget alert ([infra/README.md](../../infra/README.md)). Flat-rate: capped | Unbounded | None | Free plan: site pauses |

### Verdict

- **GitHub Pages: excluded.** Its terms forbid "commercial software as a service", which this
  becomes at V4. It also gives no private origin without a custom domain and can't set headers.
- **Netlify: excluded.** No cost or uptime advantage. Every deploy spends credits. Running out
  takes the site down.
- **Cloudflare Pages: viable, not chosen.** Cheapest at scale, but adds a vendor and a credential.
  Its address can't be wired into the stack automatically. Its uptime gives no gain, because
  the API is on AWS.
- **Amplify Hosting: viable, not chosen.** Its strengths are building from a Git branch and
  preview addresses. We have no build step, and previews can't sign in (requirement 1).
- **Recommended: S3 + CloudFront in the existing SAM template, pay-as-you-go.** It costs about $0
  at every level above. It lives in the same stack, so its address flows into Cognito and CORS
  with no hand-copied parameter. No new vendor. The flat-rate plan's spending cap is attractive,
  but its over-limit behavior is undocumented and it excludes Free Tier accounts; see C8.

## Design

### §1. Template: an optional page bucket and distribution

In [infra/template.yaml](../../infra/template.yaml):

- New parameter `HostPage` (`on` / `off`, default `off`) and condition `HostingPage`. With `off`,
  the stack is exactly as today: it points at a page on your machine via `FrontendUrl`. A dev
  stack can be hosted too (you said one will be needed); see `AlsoAllowLocalPage` below.
- New parameter `PageDomain` (default empty), used only when hosting. Empty: the page lives at
  CloudFront's own address (`xxxx.cloudfront.net`). Set (e.g. `howangryami.telotrope.ai`): see §1a.
- New parameter `AlsoAllowLocalPage` (`on` / `off`, default `off`), used only when hosting. With
  `on`, `FrontendUrl` is accepted *in addition to* the hosted address, so a hosted dev stack still
  works with a page served from your machine. A production stack leaves it `off` (C6).
- New resources, all under `HostingPage`:
  - `PageBucket`: private S3 bucket, all public access blocked (same settings as
    `RawUploadsBucket`).
  - `PageOriginAccessControl`: CloudFront's signed-request access to the bucket. The bucket is
    never readable directly.
  - `PageBucketPolicy`: allows reads only by the CloudFront service, and only for this
    distribution.
  - `PageDistribution`: CloudFront distribution. `DefaultRootObject: index.html`, so the page is served at `/` (§3 uploads `timeline.html` under that name). Viewer
    traffic redirected to HTTPS. Compression on. Managed response-headers policy
    `SecurityHeadersPolicy` (HSTS, `X-Content-Type-Options: nosniff`, frame and referrer
    headers). Its cache policy keeps files at the edge for a day (`MinTTL` 86400) while browsers
    are told to recheck every load (C4, C7). Default price class (all edge locations); within
    the free allowance it costs the same.
- The allowed-address lists are derived in one place instead of three copies of the origin
  expression. They feed Cognito's return and sign-out addresses
  ([line 229](../../infra/template.yaml#L229)), the upload bucket's CORS
  ([line 146](../../infra/template.yaml#L146)) and the API's CORS
  ([line 442](../../infra/template.yaml#L442)):

  | Stack | Cognito return addresses | CORS origins |
  |---|---|---|
  | Not hosted | `FrontendUrl` | `FrontendUrl`'s origin |
  | Hosted | `https://<host>/` | `https://<host>` |
  | Hosted, `AlsoAllowLocalPage=on` | both of the above, plus `FrontendUrl` | both origins |

  `<host>` is `PageDomain` when set, otherwise the distribution's own domain. Only `/` is listed:
  the page asks Cognito to return to the exact path it was opened at
  ([frontend/infra/cognito-login.js (line 27)](../../frontend/infra/cognito-login.js#L27)), and `/`
  is the only address we give out (C12).
- New outputs (only when hosting): `PageUrl`, `PageBucketName`, `PageDistributionId`, and
  `PageDnsTarget` (the distribution's own domain, which the custom domain's DNS record points to).

### §1a. Custom domain at Porkbun

`telotrope.ai`'s DNS is at Porkbun, and it stays there: no Route 53, no $0.50/month. With
`PageDomain` set, the template adds:

- `PageCertificate`: a free public certificate from AWS Certificate Manager for `PageDomain`,
  validated by DNS. CloudFront accepts certificates only from `us-east-1`, which is the region
  the dev stack already uses ([infra/samconfig.toml](../../infra/samconfig.toml) is git-ignored;
  I read it on this machine), so the certificate can live in the same template. A stack in any
  other region would need it created separately; the template refuses that combination with a
  rule rather than failing midway.
- `Aliases: [PageDomain]` and the certificate on `PageDistribution`.

A deploy with a new `PageDomain` pauses until the certificate is validated. The steps, written
into [infra/README.md](../../infra/README.md) (§4):

1. While the stack is creating the certificate, read the validation record (a `CNAME` name and
   value) from the CloudFormation events or the Certificate Manager console. I recall that
   CloudFormation prints it in the event's status message; check H8 confirms.
2. Add that `CNAME` at Porkbun (*Domain Management → DNS*). The deploy then continues.
3. After the deploy, add a second `CNAME` at Porkbun: `PageDomain`'s subdomain (e.g. `howangryami`)
   → the `PageDnsTarget` output.

Both records are added by hand. Porkbun has an API that could do it, but that means storing a
Porkbun API key; not worth it for a step done once per stack. The name is a deploy-time
parameter, not written into the template, so choosing it doesn't block the work (open question 1).
A separately registered domain at Porkbun works the same way: the certificate covers whatever
`PageDomain` says.

No dependency cycle: the distribution refers to nothing in Cognito or the API. They refer to it.

### §2. The hosted page knows its deployment

Today the page picks a deployment from `?deploy=<name>`, remembered in the browser
([frontend/infra/deploy-config-file.js (line 16)](../../frontend/infra/deploy-config-file.js#L16)).
A visitor should not need that. The publish script (§3) adds
`<meta name="timeline-deploy" content="<stage>">` to the uploaded copy of `timeline.html` (uploaded as `index.html`, §3).
`chosenDeployName()` reads it first. When it is present and passes `isDeployName`, it is the
answer, and `?deploy=` and the remembered choice are ignored. When it is present but invalid, the
page shows the same error a bad `?deploy=` name gives today. The repository's `timeline.html`
never contains the tag, so local development is unchanged.

### §3. `scripts/publish-page.sh <stage>`

1. Read the stack `timeline-<stage>`'s outputs. If `PageBucketName` is missing, stop with "deploy
   with HostPage=on first."
2. Stage the files in a temporary folder from an explicit list of what to include:
   `timeline.html`, renamed to `index.html` and with the §2 tag inserted by Python, not `sed`; `frontend/**/*.js` except
   `frontend/tests/`, and `vendor/**`. Nothing else is published (no `docs/`, `backend/`,
   `real-flags.json`, other deployments' settings).
3. Write the settings file into the staged copy by running
   [scripts/write-deploy-config.sh](../../scripts/write-deploy-config.sh) with its existing
   `DEPLOY_CONFIG_DIR` override ([line 19](../../scripts/write-deploy-config.sh#L19)). Reused
   as is, not reimplemented.
4. `aws s3 sync <staged> s3://<bucket> --delete --cache-control no-cache`, with
   `--content-type` given explicitly per extension (`.js` → `text/javascript`, `.json`,
   `.html`). This doesn't rely on the CLI's guess (requirement 3).
5. `aws cloudfront create-invalidation --paths '/*'`, which tells the edge to drop its copies.
   One wildcard counts as one path; the first 1,000 paths a month are free.
6. Print `PageUrl`.

Each failing step stops the script with the AWS CLI's own message (`set -euo pipefail`, as the
existing scripts do).

### §4. Documentation

- [infra/README.md](../../infra/README.md): a "Hosting the page" step (deploy with `HostPage=on`
  and optionally `PageDomain` and `AlsoAllowLocalPage`, add the two Porkbun records, run
  `publish-page.sh`).
- Migration plan C30: re-tagged `[OPEN, cross-plan]` and pointed here. When this plan's
  deployment checks pass, it becomes `[RESOLVED]`.

## Tests

- **Publish script**: new cases in [scripts/test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh),
  using its existing stand-in `aws` command:
  - refuses when the outputs lack `PageBucketName`;
  - the exact staged file list (no tests, no other deployments' settings);
  - the tag is inserted once and its value is the stage;
  - `sync` gets `--delete`, `no-cache` and the right content types;
  - invalidation names the distribution ID from the outputs.

  The stand-in's answers follow the CLI's documented output shapes and are not captured from AWS.
  The script's existing header says the same for the current cases.
- **Page**: a browser test in [e2e/](../../e2e/) that serves `timeline.html` with the tag
  injected (Playwright request interception). It checks that the page fetches that deployment's
  settings, ignores `?deploy=other`, and errors on an invalid tag value.
- **Template**: [scripts/check-template.sh](../../scripts/check-template.sh) validates the
  template with `HostPage` off, on, on with `PageDomain`, and on with `AlsoAllowLocalPage`.

### Deployment checks (only a real deployment can confirm these)

| # | Check |
|---|---|
| H1 | `PageUrl` loads the page; the bucket's own S3 address answers 403 (forbidden). |
| H2 | Every `.js` file arrives as `text/javascript`, and the modules run. |
| H3 | Sign-in through Cognito returns to `PageUrl`. |
| H4 | Loading, uploading and flag edits pass CORS from the hosted origin. |
| H5 | After a second publish, a reload shows the new version (invalidation, and browsers honoring `no-cache` despite the edge's one-day `MinTTL`; see C7). |
| H6 | Responses are compressed (`content-encoding: br` or `gzip`) and carry the security headers. |
| H7 | After a week, Cost Explorer shows CloudFront and the page bucket at $0.00. |
| H8 | With `PageDomain` set: the validation record appears in the stack's events, the deploy finishes once it's added at Porkbun, and `https://<PageDomain>/` serves the page with a valid certificate. |
| H9 | Sign-in from `https://<host>/` returns to `/`, and the page's files load from below `/` (`/frontend/main.js`, `/vendor/...`). |
| H10 | A hosted dev stack with `AlsoAllowLocalPage=on` accepts the page from your machine as well as the hosted one. |

Until H1–H10 pass on a real stack, the status is "code-level only, end-to-end TBD."

## Out of scope

- **A Content-Security-Policy header** (a browser rule listing where the page may load code and
  styles from). See C5.
- **Publishing from CI** on every merge. It needs a GitHub-to-AWS trust role. It's worth doing
  once there is a production stack.

## Open questions for you

1. The app's address. You're considering the name "How angry am I", so for example
   `howangryami.telotrope.ai` (production) and `dev.howangryami.telotrope.ai` (dev). Not needed
   before coding: it's only a deploy-time setting. Renaming the app itself (page title, the
   `timeline-<stage>` stack names) is not part of this plan.

Answered 2026-10-02: DNS is at Porkbun (§1a); a hosted dev stack will be needed at some point
(`AlsoAllowLocalPage`, §1); the page being publicly reachable is fine, since it's how people
will find the product and pay (C11).

## Self-critique log

### C1 [RESOLVED]: First draft compared bandwidth, which isn't the limit that binds
Original concern: free tiers were compared by GB. At 83 KB a visit, bandwidth stays tiny, but
38 requests a visit exhaust request allowances first. **Resolution:** the cost table is computed
from requests ([Traffic assumptions (line 45)](2026-10-02-page-hosting.md#L45)).

### C2 [RESOLVED]: Uptime comparison implied a non-AWS host could add resilience
Original concern: comparing SLAs alone suggests Cloudflare's 100% beats CloudFront's 99.9%. The
page can't work without the AWS API, so a separate host adds a failure point and removes none.
**Resolution:** stated in [Uptime (line 73)](2026-10-02-page-hosting.md#L73).

### C3 [RESOLVED]: Preview deploys were counted as an advantage
Original concern: Amplify, Netlify and Cloudflare's per-branch previews looked like a plus, but
each preview address would need to be in Cognito's and both CORS lists. **Resolution:**
requirement 1 ([line 29](2026-10-02-page-hosting.md#L29)) and the Amplify verdict
([line 110](2026-10-02-page-hosting.md#L110)).

### C4 [OPEN]: No build step means no versioned file names, so browsers recheck all 38 files on every visit
With `no-cache`, each repeat visit makes 38 small "has this changed?" requests. That adds delay
and counts against request allowances. **Mitigation in plan:** the edge answers these quickly.
They are free on pay-as-you-go up to 10 million a month. **Open:** a build step that adds
content hashes to file names, so files could be cached for a year. Trigger: page-load complaints,
or monthly requests passing 5 million.

### C5 [OPEN]: No Content-Security-Policy
The page loads Google Fonts and has a large inline `<style>` block
([timeline.html (line 12)](../../timeline.html#L12)), so a policy needs care. **Mitigation in
plan:** the managed security-headers policy covers HSTS, content-type sniffing and framing.
**Open:** a CSP. Trigger: before anyone outside the team signs in, and certainly before V4
payment.

### C6 [RESOLVED]: A hosted stack would refuse a page served from localhost
Original concern: with one derived address, you can't point a local page at a hosted stack.
**Resolution:** hosting is opt-in per stack (`HostPage`, default `off`), so unhosted dev stacks
keep the local page. Listing both addresses on every hosted stack was rejected: it would let any
page on any machine's `localhost:8000` call a production API. **Revised after your answer** that
a hosted dev stack will be needed: `AlsoAllowLocalPage` lists both, per stack, off by default
([§1 (line 119)](2026-10-02-page-hosting.md#L119)).

### C7 [OPEN]: The caching setup relies on CloudFront behavior I recalled, not checked
I believe that a cache policy's `MinTTL` keeps files at the edge even when the file says
`no-cache`, while the `no-cache` header still reaches browsers. I recalled that from AWS
documentation and did not re-read it. **Mitigation in plan:** publishing always invalidates, so
the edge side is safe either way. **Open:** deployment check H5. If browsers cache anyway, fall
back to `MinTTL` 0.

### C8 [OPEN]: Pay-as-you-go has no spending cap under a traffic flood
Beyond 10 million requests and 1 TB a month, charges are per use. The flat-rate plans cap the
bill, but the free one's over-limit behavior is undocumented and Free Tier accounts can't use it.
**Mitigation in plan:** the existing $10 budget alert ([infra/README.md](../../infra/README.md)).
**Open:** switching to a flat-rate plan (subscribable from CloudFormation since September 2026).
Trigger: the budget alert fires, or before public launch.

### C9 [RESOLVED]: GitHub Pages looked like the cheapest, simplest option
Original concern: it was the first suggestion in conversation. **Resolution:** its terms forbid
SaaS use, its origin is shared, and it can't set headers. It's excluded in
[Verdict (line 101)](2026-10-02-page-hosting.md#L101).

### C10 [OPEN]: No measured uptime for any host
All uptime figures are SLA promises or provider claims. **Open:** an external uptime check on
`PageUrl` and the API. Trigger: the first stack with users outside the team.


### C11 [RESOLVED]: Whether a publicly loadable page is acceptable
Original concern: anyone can load the page before launch. **Resolution:** you said it's fine;
the public page is how people reach the product. Data stays behind the sign-in. Removed from
the open questions ([line 275](2026-10-02-page-hosting.md#L275)).

### C12 [RESOLVED]: Opening the site at `/` would break sign-in
Original concern: the distribution served `timeline.html` at `/`, but the page asks Cognito to
return to the exact path it was opened at, and only `/timeline.html` was listed. A visitor typing
just the domain would be refused after signing in. First resolution: list both paths. **Revised
after your direction** that the file can live at `/` and needn't keep its name: the publish script
uploads it as `index.html`, the distribution serves it at `/`, and only `/` is listed
([§1 table (line 150)](2026-10-02-page-hosting.md#L150), [§3 (line 205)](2026-10-02-page-hosting.md#L205)).
Someone who types `/index.html` by hand can load the page but not sign in; nothing links there.
Check H9.

### C13 [OPEN]: The custom-domain deploy waits on a hand-added DNS record
The first deploy with a `PageDomain` stalls until the validation `CNAME` is added at Porkbun, and
the record's location in CloudFormation's output is from memory. **Mitigation in plan:** the
README steps (§1a). Certificate Manager also shows the record. **Open:** how long CloudFormation
waits before failing, if you're slow to add it. Trigger: check H8 on the first custom-domain
deploy.

## Sources

- [GitHub Pages limits and prohibited uses](https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits)
- [What is GitHub Pages (plan availability)](https://help.github.com/articles/what-is-github-pages)
- [GitHub Enterprise SLA (deprecated page)](https://docs.github.com/en/site-policy/site-policy-deprecated/github-enterprise-service-level-agreement)
- [CloudFront pricing (flat-rate plans)](https://aws.amazon.com/cloudfront/pricing/)
- [CloudFront pay-as-you-go pricing](https://aws.amazon.com/cloudfront/pricing/pay-as-you-go)
- [CloudFront flat-rate plans via API/CloudFormation, Sept 2026](https://aws.amazon.com/about-aws/whats-new/2026/09/cloudfront-flat-rate-pricing-plans-api/)
- [Flat-rate plan quotas: three free plans per account, not for Free Tier accounts](https://dev.to/aws-builders/new-pricing-model-for-cloudfront-213k) (secondary source)
- [CloudFront SLA](https://aws.amazon.com/cloudfront/sla)
- [Amplify pricing](https://aws.amazon.com/amplify/pricing/) and [Amplify SLA](https://aws.amazon.com/amplify/sla/)
- [Cloudflare Pages limits](https://55041f86.previews.developers.cloudflare.com/pages/platform/limits/index.md), [free tiers compared, Sept 2026](https://flaviocopes.com/hosting-free-tiers/)
- [Cloudflare plans](https://www.cloudflare.com/plans/network-cdn.md)
- [Netlify free plan limits, 2026](https://netli.fyi/blog/netlify-free-plan-limits-2026), [Netlify uptime thread](https://answers.netlify.com/t/uptime-on-free-tier/1123)
