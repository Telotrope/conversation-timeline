## Context

`timeline.html` is currently a 66,767-line, 730KB, 100%-client-side single HTML file — no
backend, nothing uploaded, everything computed in the browser (this is a load-bearing,
repeatedly-stated principle in [timeline-project-decisions.md:12-28](timeline-project-decisions.md#L12),
even stated directly in the tool's own UI copy). That architecture has hit three real walls:

1. **The existing "Classify with AI" feature is structurally broken outside a live Claude
   artifact.** It calls `https://api.anthropic.com/v1/messages` directly from browser JS
   ([timeline.html:65465-65520](timeline.html#L65465)) — this only works because the Claude-artifact
   host proxies/authorizes the call; a downloaded copy of the file gets a hard CORS rejection with
   no client-side workaround, documented as a known limitation at
   [timeline-project-decisions.md:388-393](timeline-project-decisions.md#L388).
2. **There is no real persistence.** The only save mechanism is
   `window.storage` (artifact-only) plus manual "download annotated JSON" — there's no way to
   pick up a session from another device, or to avoid re-uploading a 60MB file every time.
3. **There is no way to charge for the service**, and Bedrock/LLM inference costs money per call.

The user has explicitly decided to override the "100% client-side" principle: move to AWS, put
all non-presentation logic in a Rust backend, persist data server-side, do emotion/tone
classification via Amazon Bedrock (fixing wall #1 directly), and charge $5 per user to cover
costs. This plan lays out a graduated (V1–V5) migration so each step is independently shippable
and testable, rather than one big-bang rewrite. **This is a real architectural fork, not an
incremental tweak — flagging it here per the project's own instruction not to silently drop a
previously-stated principle.** The "nothing is uploaded anywhere" UI copy will need to be
rewritten starting at V2 (the first version that actually uploads data), and the privacy posture
of the tool genuinely changes: conversation content now leaves the browser, is stored (S3 +
DynamoDB), and is sent to a third-party inference service (Bedrock).

## Decisions already confirmed with the user

- **Product gating for the $5 charge**: free heuristic tier (dictionary caps + sentiment/keyword
  criticism-anger, ~$0 marginal cost) always available; **one Bedrock-quality classification pass
  per $5 payment**, additional passes require additional payment.
- **Auth**: Amazon Cognito User Pools (JWT authorizer integration with API Gateway, no hand-rolled
  password/session/reset-flow code).
- **Sentiment lexicon**: swap AFINN (ODbL-licensed, share-alike, not on this project's approved
  MIT/BSD/Apache-2.0/ISC list) for **VADER** (original `cjhutto/vaderSentiment` project is
  MIT-licensed) — requires a recalibration pass, since VADER's compound score (-1..1) is a
  different scale than AFINN's summed word scores.
- **Infrastructure-as-code**: AWS SAM (Apache-2.0) — YAML/CloudFormation-based, no separate host
  language needed (unlike CDK, which has no Rust construct language), fits a
  Lambda+API-Gateway+S3+DynamoDB+Cognito stack directly.

---

## 1. Target Architecture

### 1.1 Compute: AWS Lambda everywhere
Usage is bursty (upload → process → classify → review → export, then idle) — Lambda scales to
zero, which is what actually maps to a one-time $5 charge; Fargate/EC2 would bill for idle
capacity between sessions. Parsing/dedup of a 60MB export is a single-pass, in-memory operation
well inside Lambda's limits. The one risk is a *full Bedrock classification run* exceeding
Lambda's 15-minute ceiling (existing code batches 30 messages/call,
[timeline.html:65435](timeline.html#L65435); ~2,200 messages ≈ 74 batches). **Mitigation**:
checkpoint after every batch (mirrors the existing "save every 5 batches" resilience pattern,
[timeline-project-decisions.md:357-363](timeline-project-decisions.md#L357)) and drive remaining
batches via one SQS message per batch-offset, so the Lambda re-invokes itself per batch instead of
looping past the timeout — no Step Functions needed for this size of job.

### 1.2 API layer: API Gateway HTTP API
Cheaper than the REST API product ($1.00/million requests vs. $3.50/million), and gives shared
JWT-authorizer wiring and per-route throttling for free — needed once free vs. paid routes exist.
The Stripe webhook route is the one exception: it bypasses the JWT authorizer (Stripe can't
present a user token) and verifies the Stripe signature header inside the handler instead.

### 1.3 Storage: S3 + DynamoDB hybrid
**Pure DynamoDB doesn't work**: hard 400KB per-item limit vs. a 60MB export. **Pure S3 doesn't
work either**: the Review tab's core interaction is toggling one checkbox on one message
([timeline-project-decisions.md:253-276](timeline-project-decisions.md#L253)) — if the only store
is one giant S3 object, every checkbox click becomes a read-modify-write of the entire file
(re-introducing the exact "hold multiple full copies in memory" problem already fixed once,
[timeline-project-decisions.md:380](timeline-project-decisions.md#L380)).

| Data | Store | Shape | Why |
|---|---|---|---|
| Raw uploaded `conversations.json` | S3 (`raw/{user_id}/{upload_id}.json`), SSE encrypted | Blob | Client uploads directly via presigned PUT, bypassing API Gateway's 10MB sync payload cap entirely. |
| Annotated export | S3 (`export/{user_id}/{upload_id}.json`), generated on demand | Blob | Served back via presigned GET. |
| Per-message flags (`_claude_timeline_auto` / `_claude_timeline_user`) | DynamoDB `MessageFlags`, PK `user_id#conversation_id`, SK `message_id` (keeps the existing `${conversationIndex}|${created_at}` composite key, [timeline-project-decisions.md:89-96](timeline-project-decisions.md#L89)) | ~2,200 small items/user | Cheap point reads/writes for per-row edits; auto/user modeled as genuinely separate attributes (see §4.1). |
| Conversation/upload metadata | DynamoDB `Conversations`, PK `user_id`, SK `upload_id#conversation_index` | Small items | Powers Conversations tab/Calendar without touching the S3 blob. |
| User/account/payment state | DynamoDB `Users`, PK `user_id` | Small items | Stripe customer ID, `paid_passes_remaining` counter (see §7.2). |
| Session/block boundaries (`buildBlocks`/`attachFlags`, [timeline.html:65229-65296](timeline.html#L65229)) | **Not persisted** | N/A | Deterministic, cheap to recompute from message timestamps already in DynamoDB; avoids a cache-invalidation problem every time a flag changes. |

### 1.4 Auth: Amazon Cognito User Pools
Confirmed. JWT authorizer on the API Gateway HTTP API; no custom password/session/token code to
write and hold to the project's 100%-coverage testing bar. **Open item to verify at
implementation time**: Cognito's current free-tier MAU allowance (not verified against a live
pricing page in this planning pass — re-check before treating "free at this scale" as settled).

### 1.5 IaC: AWS SAM
Confirmed. `infra/template.yaml` is the single source of truth for every AWS resource listed
below across V2–V5.

### 1.6 Upload path
1. Client calls `POST /uploads` (authenticated) → Lambda writes an `Uploads` metadata row
   (`pending`), returns an S3 presigned PUT URL.
2. Client `PUT`s the raw export directly to S3 (never through Lambda/API Gateway).
3. An S3 `ObjectCreated` event triggers a processing Lambda: `unwrap_uploaded_json` +
   `dedup_chat_messages` (ports of [timeline.html:64953-64990](timeline.html#L64953)) run here,
   writing per-message rows to `MessageFlags` and metadata to `Conversations`.
4. Client polls `GET /uploads/{id}` until `status: ready`.

---

## 2. Rust Crates (licenses stated)

| Purpose | Crate | License |
|---|---|---|
| Web framework | `axum` | MIT |
| Lambda runtime/HTTP adapter | `lambda_http`, `lambda_runtime` (`aws-lambda-rust-runtime`, GA) | Apache-2.0 |
| Build/deploy | `cargo-lambda` | Apache-2.0 |
| Async runtime | `tokio` | MIT |
| AWS SDK — S3, DynamoDB, Bedrock Runtime, Cognito | `aws-sdk-s3`, `aws-sdk-dynamodb`, `aws-sdk-bedrockruntime` (`converse` API, model-agnostic — preferred over raw `invoke_model`), `aws-sdk-cognitoidentityprovider` | Apache-2.0 |
| Serialization | `serde`, `serde_json` | MIT OR Apache-2.0 |
| Timezone-aware bucketing | `chrono`, `chrono-tz` | MIT OR Apache-2.0 |
| Sentiment lexicon | Port the MIT-licensed VADER lexicon/algorithm directly from `cjhutto/vaderSentiment`, or a permissively-licensed Rust port if one is verified at implementation time (check crates.io for an actively-maintained MIT/Apache VADER crate before hand-porting) | MIT |
| Stripe integration | `async-stripe` + `async-stripe-webhook` (pre-1.0 release candidates as of this writing — pin exact versions, expect API churn) | MIT OR Apache-2.0 |
| HTTP mocking for tests | `wiremock` | MIT |
| Property-based testing | `proptest` | MIT OR Apache-2.0 |
| Snapshot testing | `insta` | Apache-2.0 |
| Test runner | `cargo-nextest` | Apache-2.0 OR MIT |
| Local AWS emulation | LocalStack Community Edition (S3 + DynamoDB only — see §2.1) | Apache-2.0 |

### 2.1 LocalStack cannot emulate Bedrock in the free tier
Verified directly: Bedrock model emulation is LocalStack **Pro** (paid), not in the Apache-2.0
Community Edition, and even there it proxies to a local model server rather than truly emulating
Bedrock's behavior/latency/errors. Combined with this project's own testing convention (verified
real communication samples required for interfaces with other programs, not synthetic mocks
alone), V3's test strategy is: `wiremock`-based unit tests for prompt-building/response-parsing,
**one real captured-and-sanitized Bedrock `Converse` request/response pair** checked in as a
fixture, and a small number of real-Bedrock integration tests run manually/on a schedule (not
per-PR, given real cost). **This requires briefly using a real AWS account with Bedrock model
access enabled — needs your sign-off before V3 starts** (see C6 below).

---

## 3. Version-by-Version Breakdown

### V1 — Rust core logic, no AWS yet (pure port + parity proof)
**Adds**: `backend/timeline-core` library crate, pure functions, no I/O:
- `dedup_chat_messages` — port of [timeline.html:64906-64940](timeline.html#L64906).
- `unwrap_uploaded_json` — port of [timeline.html:64953-64961](timeline.html#L64953) (format-v2
  wrapper detection).
- `build_blocks` — port of [timeline.html:65229-65264](timeline.html#L65229), **UTC in, UTC out**;
  day-bucketing deliberately excluded (see §4.3).
- ALL-CAPS dictionary check — port of the §5.1 logic in
  [timeline-project-decisions.md:222-233](timeline-project-decisions.md#L222).
- Criticism keyword regex — port of the wide-net phrase list,
  [timeline-project-decisions.md:242-243](timeline-project-decisions.md#L242).
- Anger detection — **re-implemented against VADER** instead of AFINN
  ([timeline.html:64860](timeline.html#L64860) is the AFINN table being replaced), keeping the
  existing anger-specific phrase list and exclamation-mark-burst logic
  ([timeline-project-decisions.md:246-249](timeline-project-decisions.md#L246)), with thresholds
  recalibrated against the same hand-curated baseline the AFINN version was tuned against.
- `effective_flag` four-state matrix — port of
  [timeline.html:65193-65209](timeline.html#L65193), for server-side Analytics aggregates.

**Stays client-side**: all rendering (SVG charts, markdown-lite renderer, tab UI). No network
calls exist yet — this version is a tested library, not a deployed service.

**AWS resources**: none.

**Tests**:
- Unit tests: the 6 hand-built dedup edge cases named in
  [timeline-project-decisions.md:66-70](timeline-project-decisions.md#L66) (simple chains, a stray
  reply to an early attempt, no duplicates, mid-conversation duplicates, a 5-way chain with a stray
  reply), ported as literal Rust cases.
- `proptest` property test: for any generated human/assistant message sequence, deduped output
  never has two adjacent-in-the-human-subsequence identical messages, and message count only
  decreases.
- Regression test against a real sample dataset, now checked in at
  [backend/tests/fixtures/sample_conversations.json](backend/tests/fixtures/sample_conversations.json)
  (see C1 for provenance and a discrepancy against the decisions doc's original stat that this
  surfaced): 6 conversations, 36 raw messages, **6 dropped by dedup, 30 remain** — 3 of the 6
  conversations (`IRS TIN match failure on sam.gov`, and windowed excerpts of `Starting a
  government contracting business` and `401k benefit administration for small businesses`) each
  contain one real resend-after-empty-assistant-reply duplicate pair, which is exactly the
  real-world case the dedup logic exists for.
- ALL-CAPS dictionary test: `IRS`/`DARPA`/`ICHRA`/`QSEHRA`/`OK` → zero matches;
  `WRONG`/`RIDICULOUS` → flagged ([timeline-project-decisions.md:232-233](timeline-project-decisions.md#L232)).
- VADER recalibration test: run the new anger detector against the same hand-curated
  criticism/anger baseline the AFINN version was checked against
  ([timeline-project-decisions.md:243-249](timeline-project-decisions.md#L243)); recall should not
  regress below the AFINN-based baseline's recall.
- `insta` snapshot test on `build_blocks` for a synthetic multi-day, multi-gap sequence, asserting
  session boundaries land exactly on the ≥15-minute gap rule.

### V2 — Deployed backend: storage, auth, upload/review flow (no Bedrock, no payment)
**Adds**: Lambda handlers (`axum` + `lambda_http`) behind API Gateway: `POST /uploads`
(presigned-URL issuance), the S3-triggered processing Lambda, `GET /conversations`,
`GET`/`PATCH /messages/{id}/flags` (two-field auto/user write path, structurally enforced — §4.1),
`GET /export`. Cognito gates all routes. `timeline.html` is refactored to `fetch()` these
endpoints instead of parsing a local file
([timeline.html:64963 onward](timeline.html#L64963) call sites removed); `window.storage`
auto-save/recovery ([timeline.html:65298-65365](timeline.html#L65298)) is retired — it was only
ever a fallback for the artifact-hosting context.

**AWS resources**: S3 bucket (raw + export prefixes, SSE), DynamoDB `Users`/`Conversations`/
`MessageFlags`, Lambda functions per route, API Gateway HTTP API, Cognito User Pool + app client,
per-function least-privilege IAM roles.

**Tests**:
- Unit tests: request/response (de)serialization per route; a direct unit test asserting the
  auto-write code path's DynamoDB `UpdateExpression` never references the user attribute (and
  vice versa) — turning the auto/user separation from convention into something a test enforces.
- Integration tests against **LocalStack Community Edition** (S3 + DynamoDB, confirmed free):
  docker-compose'd in CI, exercising the real upload → S3-event → processing Lambda → DynamoDB
  pipeline end to end.
- A large-payload test: a real ~60MB upload (the sample file once supplied, or a synthetic one of
  equivalent shape) completes within Lambda memory/time budgets and never exceeds DynamoDB's
  400KB item cap.
- Auth test: an unauthenticated/invalid-JWT request is rejected at the authorizer layer, verified
  against a real test Cognito user pool (Cognito is not part of LocalStack's free-tier coverage).
- Also run the full suite once against real (low-volume) AWS S3+DynamoDB before calling V2 done,
  since LocalStack fidelity is the main risk in this version.

### V3 — Bedrock-based classification
**Adds**: server-side port of `classifyBatchWithAI`/`classifyBatchWithRetry`
([timeline.html:65417-65531](timeline.html#L65417)) calling `aws-sdk-bedrockruntime`'s `converse`
API instead of a client-side `fetch()` to `api.anthropic.com` — the direct fix for the CORS/no-API-
key dead end at [timeline-project-decisions.md:388-393](timeline-project-decisions.md#L388). Batch
orchestration via the SQS-checkpoint design (§1.1). **Prompt hardening is preserved verbatim**:
the XML `<message index="N">` tags and `escapeForPromptTags`
([timeline.html:65428-65464](timeline.html#L65428)) carry over unchanged, not redesigned.

**Files/modules**: `backend/timeline-core/src/classify.rs` (prompt building, pure/testable),
`backend/timeline-api/src/bedrock.rs` (SDK call + retry/checkpoint), new SQS queue, new
`ClassificationRuns` DynamoDB table (mirrors the "save every 5 batches" pattern,
[timeline-project-decisions.md:357-363](timeline-project-decisions.md#L357)).

**AWS resources**: SQS queue, Bedrock model access enabled on the account, `ClassificationRuns`
table, IAM scoped to `bedrock:Converse` on the specific model ARN.

**Tests** (per §2.1, no LocalStack Bedrock emulation available):
- `wiremock` unit tests: prompt-building snapshot tests (`insta`), and response-parsing tests
  covering every documented failure mode at
  [timeline.html:65478-65519](timeline.html#L65478) (network error, non-JSON response, API-level
  error, array-length mismatch, non-array response) — these were real bugs once
  ([timeline-project-decisions.md:369-382](timeline-project-decisions.md#L369)) and must not
  regress.
- One verified real communication sample: a captured `Converse` request/response pair from a real
  dev-account Bedrock call, personal content replaced with synthetic-but-structurally-identical
  text, checked in at `backend/tests/fixtures/bedrock_converse_sample.json`.
- A small number of real-Bedrock integration tests, run manually/nightly given per-call cost.
- Regression tests: retry-once-per-batch, abort-after-first-systemic-failure, positional-partial-
  application ([timeline-project-decisions.md:305-310](timeline-project-decisions.md#L305)) ported
  as literal cases against the mocked HTTP layer.

### V4 — Payment ($5 charge) and product gating
**Adds**: Stripe Checkout session creation (`POST /checkout`), webhook endpoint
(`POST /webhooks/stripe`, signature-verified via `async-stripe-webhook`, bypasses the JWT
authorizer per §1.2), and entitlement gating on the classify-trigger route implementing the
confirmed gating model: free heuristic tier always available; starting a Bedrock classification
run requires `Users.paid_passes_remaining > 0`, decremented via a DynamoDB **conditional** update
(prevents a double-spend race between two concurrent classify requests); a completed $5 Stripe
payment increments the counter by 1.

**Files/modules**: `backend/timeline-api/src/payments/stripe.rs`; `Users` table gains
`stripe_customer_id`, `paid_passes_remaining`, `payment_history`.

**AWS resources**: none new beyond existing API Gateway/Lambda (webhook is just another route);
Stripe account (test + live mode).

**Tests**:
- Unit tests: webhook signature verification (valid/invalid/replayed/tampered), idempotency (the
  same `checkout.session.completed` event ID processed twice must not double-grant a pass).
- Stripe test mode is free, so — unlike Bedrock — integration tests **do** hit the real Stripe
  test-mode API in CI (test API key as a CI secret), per this project's "communicate with the
  actual external program" convention.
- One real captured sample: a Stripe-CLI-triggered test-mode webhook payload (`stripe trigger
  checkout.session.completed` via `stripe listen`), checked in as a fixture for signature-
  verification unit tests.
- Replay-attack test: a captured payload with a valid-but-stale timestamp/signature is rejected
  (Stripe's tolerance window enforced).
- Conditional-decrement race test: two concurrent classify-start requests against
  `paid_passes_remaining = 1` — exactly one succeeds.

### V5 — Production hardening: load, security, chaos
**Adds**: API Gateway usage plans (rate-limiting free-heuristic calls harder than paid Bedrock
calls), CloudWatch cost-anomaly and error-rate alarms, WAF on the API Gateway stage, a documented
rollback plan, and re-examining the decisions doc's remaining open items now that real Bedrock
classification exists at scale — specifically the ≈0 session-length/flag-rate correlation flagged
as "not yet re-tested end-to-end"
([timeline-project-decisions.md:400-402](timeline-project-decisions.md#L400)).

**AWS resources**: WAF WebACL, CloudWatch alarms + budget/cost-anomaly detection, API Gateway
usage plans/API keys per entitlement tier.

**Tests**:
- Load tests simulating concurrent uploads/classification runs (tool choice — e.g. `k6` or
  `oha` — TBD at implementation time; license-check before adopting, per project convention).
- Run this repo's `/security-review` skill against the full backend diff; specifically an
  adversarial test crafting a message that looks like a `</message>` closing tag, targeting the
  exact prompt-injection bug class already found once
  ([timeline-project-decisions.md:378](timeline-project-decisions.md#L378)).
- Chaos test: inject Bedrock throttling/5xx via `wiremock` mid-batch-run, confirm checkpoint-and-
  resume actually resumes from the last good checkpoint rather than reprocessing or dropping a
  batch.

---

## 4. Preserving Correctness Across the Client/Server Split

### 4.1 Two-field auto/user flag model
Modeled as genuinely separate DynamoDB attributes (or sort-key-suffixed items,
`MSG#{id}#AUTO` / `MSG#{id}#USER`), written by two different, narrowly-IAM-scoped code paths — the
classification Lambda (heuristic in V1/V2, Bedrock in V3) can only ever touch `AUTO`; the
user-override `PATCH` route can only ever touch `USER`. This turns "auto must never overwrite
user" from a convention (as it is today, [timeline.html:64992-65005](timeline.html#L64992)) into
something a unit test can assert structurally. `effectiveFlag()`'s four-state matrix
([timeline-project-decisions.md:264-276](timeline-project-decisions.md#L264)) stays a
**client-side** pure function computed from the two already-fetched fields — the `SHOW_AUTO`/
`SHOW_USER` toggles need to feel instantaneous, and this is genuinely presentation logic (deciding
what to *display*), not new computation on raw data. Server-side Analytics aggregates (friction
ranking, flag-rate trend) use the Rust port of the same logic since those already require a round
trip.

### 4.2 Dedup semantics
Runs exactly once, server-side, at upload time, mirroring the existing format-v2 rule (bare array
= unprocessed = dedup runs; wrapped-with-version-marker = already deduped,
[timeline-project-decisions.md:81-87](timeline-project-decisions.md#L81)). The client never
re-implements this.

### 4.3 Local-timezone session bucketing
`buildBlocks()` ([timeline.html:65229-65264](timeline.html#L65229)) fuses two things with
different timezone sensitivity — split them:
1. **Gap-based session splitting** (≥15 min since previous message) is timezone-agnostic — a delta
   between two instants. **Moves to the Rust backend** as `timeline_core::build_blocks`, UTC in,
   UTC session-boundary timestamps out.
2. **Which calendar day a session renders under** (`localDateKey()`,
   [timeline.html:65222-65227](timeline.html#L65222)) is timezone-sensitive, and is exactly the
   logic whose earlier server-side-in-UTC implementation caused a real bug ("Calendar bars ran
   past the edge of their day,"
   [timeline-project-decisions.md:371](timeline-project-decisions.md#L371)). **Stays client-side**:
   the browser buckets backend-computed UTC session boundaries into calendar days using its own
   timezone, exactly as today.
3. **Exception**: server-side day/week-bucketed Analytics aggregates need *some* notion of "which
   day." The client sends its IANA timezone name (`Intl.DateTimeFormat().resolvedOptions().timeZone`)
   as a request parameter; the backend uses `chrono-tz` to bucket by that name for that response
   only — never cached or persisted, since a stored numeric offset would silently misbucket
   sessions across a DST transition.

### 4.4 Prompt-injection hardening (Bedrock-side)
The trust boundary is explicit: untrusted user message text → Bedrock prompt text. The existing
XML-tag-delimiter + escaping approach
([timeline.html:65428-65464](timeline.html#L65428),
[timeline-project-decisions.md §5.4/§10](timeline-project-decisions.md#L278)) is preserved
verbatim in the Rust port, plus the V5 adversarial test named above.

---

## 5. License Notes (data assets, not just crates)

- **The ~64,000-word English dictionary** ([timeline.html:930-64830](timeline.html#L930), sourced
  from Debian's `wamerican`/SCOWL): SCOWL's grant permits use/copy/modify/distribute/sell "for any
  purpose without fee" — fine for a paid product, no action needed.
- **AFINN → VADER swap**: resolved per the decisions above (§ "Decisions already confirmed").

---

## 6. Repository Layout

### 6.1 Crate breakdown — one crate per bounded responsibility

`backend/` is a Cargo **workspace** of several small crates, not one crate with many files. This
is what makes pieces "easily separable" in a concrete, checkable way, not just a naming
convention: a crate boundary is a real compile-time boundary, so a piece can't accidentally reach
into another piece's internals, and any one crate could later be pulled into its own repo/service
with no refactor beyond updating its `Cargo.toml` path dependency.

| Crate | Responsibility | Depends on |
|---|---|---|
| `timeline-core` | Pure domain logic: dedup, session/block splitting, flag heuristics, classify-prompt building, the four-state flag matrix. **No AWS SDK, no HTTP, no I/O of any kind.** Also defines the trait "ports" (see §6.2) that infra crates implement. | nothing but `serde`/`chrono` |
| `timeline-storage` | S3 + DynamoDB adapters implementing `timeline-core`'s storage ports. | `timeline-core` |
| `timeline-bedrock` | Bedrock `converse` client, batch/checkpoint orchestration (V3). | `timeline-core` |
| `timeline-payments` | Stripe checkout + webhook verification + entitlement logic (V4). | `timeline-core`, `timeline-storage` |
| `timeline-auth` | Cognito JWT verification, an axum extractor/middleware for authenticated routes (V2). | nothing infra-specific beyond the JWT/Cognito SDK |
| `timeline-api` | axum `Router` assembly, route handlers (thin — parse request, call a `timeline-core` function or a port, serialize response), the Lambda entrypoint. | all of the above |

Dependencies only point one direction: `timeline-api` → the infra crates → `timeline-core`.
**`timeline-core` never depends on anything infra-specific.** This is a direct consequence of the
ports-and-adapters split in §6.2, and it's the thing that actually delivers "easily separable" —
`timeline-storage` could be replaced by a Postgres-backed crate without `timeline-core` or its
tests changing at all.

### 6.2 Ports and adapters — where "separable" gets enforced, not just claimed

`timeline-core::ports` defines traits for everything that needs I/O — e.g.
`trait MessageFlagsStore { fn get(...); fn set_auto(...); fn set_user(...); }`,
`trait ConversationStore`, `trait Classifier`. `timeline-storage`/`timeline-bedrock` provide the
real DynamoDB/S3/Bedrock-backed implementations; tests provide in-memory fakes. Route handlers in
`timeline-api` depend on the trait, not the concrete adapter, so a route handler's unit tests never
need LocalStack or a mock HTTP server — only the adapter's own tests do (§3's per-version test
plan already separates these: adapter tests in V2/V3 against LocalStack/real AWS, handler tests
as plain unit tests against fakes).

### 6.3 File and module rules

- **One primary type or tightly-related family per file**, file name matching the primary item
  (`dedup.rs` → `dedup_chat_messages`, not a grab-bag). No `utils.rs`/`helpers.rs` catch-all files
  — every function lives in the module that owns its concern.
- **Route handlers**: one file per resource, not one big `routes.rs` — e.g.
  `timeline-api/src/routes/{uploads,messages,conversations,classify,checkout,webhooks}.rs`.
- **Soft target: ~300–400 lines of implementation code per file.** Comfortable to review and to
  hold in your head in one pass.
- **Hard ceiling: 1,000 lines, enforced by a test, not just convention.** Add
  `backend/tests/test_file_sizes.rs` (or a workspace-level xtask) that walks every `src/**/*.rs`
  file, strips `#[cfg(test)] mod tests { ... }` blocks before counting (tests legitimately add
  bulk without hurting the implementation's readability), and fails if any file's remaining line
  count exceeds 1,000 — the same **ratchet pattern** this repo's own
  [CLAUDE.md](CLAUDE.md) already uses for
  `tests/test_no_unhandled_exceptions.py` (new violations fail, pre-existing ones are
  allowlisted). Since this backend starts from zero, the allowlist starts empty — no grandfathered
  files, ever.
- **When a file approaches the ceiling, split by seam, not by line count alone**: pull out one
  type/trait + its impls, split a large `match`/enum-dispatch block into its own module, or split
  a route file by sub-resource. A file that's merely long but is genuinely one cohesive concern is
  a smaller problem than a short file that's doing three unrelated things — the 1,000-line rule is
  a forcing function to notice the split, not a goal in itself.
- **Narrow public surface per crate**: each crate's `lib.rs` re-exports only what other crates
  need; internal modules default to `pub(crate)`. This is what keeps a crate's *internal* file
  layout free to change without breaking anything outside it — a prerequisite for "separable"
  meaning anything in practice.

### 6.4 Example layout (V1–V3 shape)

```
backend/
  Cargo.toml                        # workspace root
  timeline-core/
    src/
      lib.rs
      dedup.rs
      sessions.rs
      flags/
        heuristic.rs                # ALL-CAPS + criticism keyword + VADER anger
        matrix.rs                   # effective_flag four-state logic
      classify/
        prompt.rs                   # buildClassifyPrompt port
        response.rs                 # response parsing + the 5 documented failure modes
      ports.rs                      # MessageFlagsStore, ConversationStore, Classifier traits
  timeline-storage/
    src/
      lib.rs
      s3.rs
      dynamo/
        message_flags.rs
        conversations.rs
        users.rs
  timeline-bedrock/
    src/
      lib.rs
      client.rs
      checkpoint.rs                 # SQS-driven batch resume, §1.1
  timeline-auth/
    src/
      lib.rs
      cognito.rs
  timeline-payments/                # V4
    src/
      lib.rs
      stripe_client.rs
      webhook.rs
      entitlement.rs                # paid_passes_remaining logic, §7.2
  timeline-api/
    src/
      main.rs                       # Lambda + local-dev entrypoint
      app.rs                        # Router assembly
      routes/
        uploads.rs
        messages.rs
        conversations.rs
        classify.rs
        checkout.rs                 # V4
        webhooks.rs                 # V4
  tests/
    test_file_sizes.rs              # the 1,000-line ratchet check, §6.3
```

- `frontend/` — the presentation-only remainder of `timeline.html`. Once the dictionary/AFINN-or-
  VADER data and all business logic move server-side, the shipped client file shrinks from 730KB
  down to roughly the markup + rendering logic currently at
  [timeline.html:1-918](timeline.html#L1) and
  [timeline.html:65677 onward](timeline.html#L65677) (charts, markdown-lite renderer, tab
  switching, cross-navigation).
- `infra/` — `template.yaml` (AWS SAM).
- `timeline-project-decisions.md` stays at the repo root as the canonical constraint log; this
  plan doesn't move it.

---

## 7. Rough Cost Shape (order of magnitude, not a bill)

Using the sample dataset (~2,200 messages after dedup / 4,482 raw / 60MB,
[timeline-project-decisions.md:66-70](timeline-project-decisions.md#L66)):

| Component | One full session (upload + review + one classify pass + export) |
|---|---|
| S3 storage (~120MB) | ~$0.003/month — negligible |
| DynamoDB (~2,200 items) | ~$0.01 — negligible |
| Lambda (parsing/dedup) | fractions of a cent |
| API Gateway | fractions of a cent |
| **Bedrock classification (dominant cost)** | **~$0.70–$1 at Claude Haiku pricing** (consistent with the original design-time estimate, [timeline-project-decisions.md:295-297](timeline-project-decisions.md#L295)); meaningfully more at Sonnet-class models — re-check the exact per-token cost of whichever model ID is chosen at implementation time, pricing moves. |
| Stripe fee on $5 | ~$0.445 (2.9% + $0.30) |

**Net**: roughly $1–2 total cost against a $5 charge for one pass on this sample size — real
margin, but it narrows for larger files and is why the gating model caps each $5 to exactly one
pass rather than unlimited reruns (the existing "refresh detection" feature,
[timeline-project-decisions.md:97-101](timeline-project-decisions.md#L97), is a real, already-
designed way a user could otherwise re-trigger the expensive part for free).

**Uncertainty flags**: token-count assumptions are estimated from the existing batch design (30
messages/call, ~300-token prior-reply context, [timeline.html:65435](timeline.html#L65435)), not
measured against a real sample yet — the V3 verified-sample fixture will replace this estimate
with a real number (see C6).

---

## Self-critique log

### C1 [RESOLVED]: Sample `conversations.json` file not available in this repo
Original concern: the V1 regression test needed a real file rather than a re-asserted magic
number. **Resolution**: you supplied `conversations.json` (64.7MB, 117 conversations, 4,457 raw
messages — a real Anthropic export, bare-array format; since removed from the repo after upload,
since it was your real personal conversation content — see C9). A trimmed, format-preserving
fixture is checked in at
[backend/tests/fixtures/sample_conversations.json](backend/tests/fixtures/sample_conversations.json)
(6 conversations, 36 raw messages, 561KB), selected to include the 3 conversations in the full
export that actually exercise dedup (a resend after an empty/errored assistant reply — the exact
real-world case the dedup logic exists for) plus a 0-message and two 2-message conversations for
baseline schema coverage. Addressed in [§3 V1 tests](2026-09-09-rust-aws-backend-migration.md#L171).
**This resolution surfaced a separate, real discrepancy — see C8, tracked openly rather than
folded silently into this fix.**

### C2 [RESOLVED]: AFINN lexicon license (ODbL) vs. a monetized product
Original concern: ODbL's share-alike terms are ambiguous for a paid product and AFINN isn't on
this project's approved license list. **Resolution**: swap to VADER (MIT-licensed upstream),
addressed in the "Decisions already confirmed" section and V1's scope
([this plan §3](flickering-coalescing-puffin.md), V1 "Anger detection" bullet) — includes a
recalibration task against the existing hand-curated baseline so detection quality doesn't
silently regress.

### C3 [RESOLVED]: Auth — Cognito vs. lightweight magic-link
Original concern: Cognito setup overhead vs. hand-rolled auth's larger security-sensitive surface
to test. **Resolution**: Cognito confirmed by you, addressed in §1.4. **Residual open item**:
Cognito's current free-tier MAU threshold not verified against a live pricing page in this
planning pass — confirm at implementation time before treating "free at this scale" as settled.

### C4 [RESOLVED]: IaC tool choice
Original concern: the request's premise (OpenTofu as "the Apache-2.0 Terraform fork") was
inaccurate — OpenTofu is MPL 2.0, not on this project's approved license list, and Terraform
itself moved to the non-OSI BUSL 1.1. **Resolution**: AWS SAM (Apache-2.0) confirmed by you,
addressed in §1.5 and used as the resource source-of-truth (`infra/template.yaml`) throughout
§3's version list.

### C5 [RESOLVED]: Product gating design
Original concern: what exactly $5 unlocks was a genuine product decision with real cost-margin
consequences (§7). **Resolution**: free heuristic tier + one paid Bedrock pass per $5, confirmed
by you, implemented via `Users.paid_passes_remaining` with conditional-decrement enforcement in
V4 (§3 V4, §7).

### C6 [OPEN]: Capturing real verified-communication-sample fixtures requires real external calls
V3 needs one real captured-and-sanitized Bedrock `Converse` call (real, if small, AWS cost); V4
needs one real Stripe test-mode webhook payload (free, Stripe test mode). **Mitigation in plan**:
scoped narrowly in §2.1/§3 (V3) and §3 (V4) — Bedrock as one real dev-account call plus scheduled
(not per-PR) integration tests; Stripe as CI-integrated since test mode is free. **Open**: needs
your sign-off before V3 starts, specifically on briefly enabling Bedrock model access on a real
AWS account to capture that one sample.

### C7 [RESOLVED]: Whether LocalStack could cover V3's Bedrock testing entirely
Original concern: could the whole test pyramid stay emulator-based through V3, matching V1/V2's
LocalStack approach? **Resolution**: verified directly that Bedrock emulation is LocalStack
Pro-only, not in the free Apache-2.0 Community Edition — addressed in §2.1, which redesigns V3's
test strategy around `wiremock` unit tests plus one real captured sample plus scheduled real
integration tests.

### C8 [OPEN, likely resolved pending confirmation]: The decisions doc's dedup statistic doesn't match the real uploaded export
Running the exact `dedupChatMessages` algorithm
([timeline.html:64906-64940](timeline.html#L64906)), ported faithfully to Python and verified
against its own logic, against the full uploaded `conversations.json` gave **4,457 raw messages,
4,451 after dedup, 6 dropped across 3 of 117 conversations** — not the **31 of 4,482** stated at
[timeline-project-decisions.md:66-70](timeline-project-decisions.md#L66). Real, measured mismatch,
not run-to-run noise: same algorithm, same-shaped input, a materially different count.

**Leading hypothesis** (proposed by you, checked and quantitatively confirmed by me): the file you
gave me had already had some duplicate messages stripped before I received it. This isn't just
plausible — the arithmetic matches exactly: `4,482 raw − 31 dropped = 4,451 final`, and
`4,457 raw − 6 dropped = 4,451 final`. Both converge on the **same final (post-dedup) message
count**, and the raw-count gap (25) equals the dropped-count gap (25) one-for-one. That's a
specific quantitative match, not a rough coincidence — it's exactly the signature you'd expect if
25 of the original 31 duplicate messages were already gone before this file reached me, leaving 6
still-duplicated. **What I have not done**: confirm this directly against the original 4,482-message
file (I don't have it — see C9) or otherwise pin down *why* those 25 were already missing (an
earlier dedup pass re-exported, a change in the export mechanism, or something else). So I'm
calling this a strongly-supported inference, not a confirmed fact.
**Mitigation in plan**: none needed on the plan itself — the new fixture and V1 regression test
(§3) already use the real, freshly-measured number (6 of 36 dropped in the fixture), so V1's test
is accurate regardless of which of these explanations is right. **Open**: whether to annotate or
correct the statistic in [timeline-project-decisions.md:66-70](timeline-project-decisions.md#L66)
is your call, not mine. Trigger: your decision on whether to update that line — nothing else
depends on resolving this further.

### C9 [RESOLVED]: Real personal conversation content was sitting uncommitted in the repo
Original concern: I flagged that your uploaded `conversations.json` (real personal Claude
conversation content, 64.7MB) was sitting untracked at the repo root, and that it shouldn't be
committed to git history by accident. **Resolution**: you removed the file yourself after I
raised it. The only conversation-derived content still in the repo is the trimmed, already-scoped
fixture at
[backend/tests/fixtures/sample_conversations.json](backend/tests/fixtures/sample_conversations.json)
— if different or fresher example data is ever needed (e.g. to investigate C8 further), you'll
supply it again rather than me generating or requesting it independently.

---

## Verification (end to end, per version)

- **V1**: `cargo test` (unit + `proptest` + `insta` snapshots) inside `backend/timeline-core`;
  no deployment needed to verify this version — it's a pure-logic parity proof against the
  documented JS behavior.
- **V2**: `docker-compose up localstack`, run `cargo nextest run` against it in CI; then one manual
  run against a real low-cost AWS S3+DynamoDB pair (`sam deploy --config-env dev`) to confirm
  LocalStack fidelity before sign-off.
- **V3**: `wiremock`-backed unit tests on every commit; the one real Bedrock sample captured
  manually and checked in; a small scheduled (e.g. nightly) job hitting real Bedrock, not gating
  every PR.
- **V4**: Stripe CLI (`stripe listen`, `stripe trigger checkout.session.completed`) exercised in
  CI against Stripe test mode on every PR; a manual live-mode $5 test charge (refunded after) as
  a one-time pre-launch check.
- **V5**: `/security-review` skill run against the full backend diff; load test tool run manually
  against a staging deploy before declaring V5 done.

---

## Critical Files

- [timeline.html](timeline.html) — porting source. Key ranges:
  [64906-64990](timeline.html#L64906) (dedup + flag load), [65193-65296](timeline.html#L65193)
  (`effectiveFlag`/`buildBlocks`/`attachFlags`), [65417-65531](timeline.html#L65417)
  (`classifyBatchWithAI`/`classifyBatchWithRetry`).
- [timeline-project-decisions.md](timeline-project-decisions.md) — full constraint set; §2.3
  (dedup), §3 (sessions), §5.3 (four-state matrix), §10 (bug root causes) are most load-bearing.
- [CLAUDE.md](CLAUDE.md) — testing rigor, exception-handling, trust-boundary sanitization, and
  license-discipline rules every version's design above was checked against.
- New: `backend/timeline-core/src/{dedup,sessions,flags/heuristic,classify}.rs`,
  `infra/template.yaml`.
