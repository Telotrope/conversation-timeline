## Context

`timeline.html` is currently a 66,767-line, 730KB, 100%-client-side single HTML file — no
backend, nothing uploaded, everything computed in the browser (this is a load-bearing,
repeatedly-stated principle in [timeline-project-decisions.md:12-28](../../timeline-project-decisions.md#L12),
even stated directly in the tool's own UI copy). That architecture has hit three real walls:

1. **The existing "Classify with AI" feature is structurally broken outside a live Claude
   artifact.** It calls `https://api.anthropic.com/v1/messages` directly from browser JS
   ([timeline.html:65465-65520 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65465-L65520)) — this only works because the Claude-artifact
   host proxies/authorizes the call; a downloaded copy of the file gets a hard CORS rejection with
   no client-side workaround, documented as a known limitation at
   [timeline-project-decisions.md:388-393](../../timeline-project-decisions.md#L388).
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
[timeline.html:65435 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65435); ~2,200 messages ≈ 74 batches). **Mitigation**:
checkpoint after every batch (mirrors the existing "save every 5 batches" resilience pattern,
[timeline-project-decisions.md:357-363](../../timeline-project-decisions.md#L357)) and drive remaining
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
([timeline-project-decisions.md:253-276](../../timeline-project-decisions.md#L253)) — if the only store
is one giant S3 object, every checkbox click becomes a read-modify-write of the entire file
(re-introducing the exact "hold multiple full copies in memory" problem already fixed once,
[timeline-project-decisions.md:380](../../timeline-project-decisions.md#L380)).

| Data | Store | Shape | Why |
|---|---|---|---|
| Raw uploaded `conversations.json` | S3 (`raw/{user_id}/{upload_id}.json`), SSE encrypted | Blob | Client uploads directly via presigned PUT, bypassing API Gateway's 10MB sync payload cap entirely. |
| Annotated export | S3 (`export/{user_id}/{upload_id}.json`), generated on demand | Blob | Served back via presigned GET. |
| Per-message flags (`_claude_timeline_auto` / `_claude_timeline_user`) | DynamoDB `MessageFlags`, PK `user_id#conversation_id`, SK `message_id` (keeps the existing `${conversationIndex}|${created_at}` composite key, [timeline-project-decisions.md:89-96](../../timeline-project-decisions.md#L89)) | ~2,200 small items/user | Cheap point reads/writes for per-row edits; auto/user modeled as genuinely separate attributes (see §4.1). |
| Conversation/upload metadata | DynamoDB `Conversations`, PK `user_id`, SK `upload_id#conversation_index` | Small items | Powers Conversations tab/Calendar without touching the S3 blob. |
| User/account/payment state | DynamoDB `Users`, PK `user_id` | Small items | Stripe customer ID, `paid_passes_remaining` counter (see §7.2). |
| Session/block boundaries (`buildBlocks`/`attachFlags`, [frontend/core/blocks.js:38-67](../../frontend/core/blocks.js#L38-L67) and [frontend/core/flags.js:35-62](../../frontend/core/flags.js#L35-L62)) | **Not persisted** | N/A | Deterministic, cheap to recompute from message timestamps already in DynamoDB; avoids a cache-invalidation problem every time a flag changes. |

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
   `dedup_chat_messages` (ports of [frontend/core/export-format.js:26-94](../../frontend/core/export-format.js#L26-L94) and [timeline.html:64906-64940 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L64906-L64940)) run here,
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
- `dedup_chat_messages` — port of [timeline.html:64906-64940 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L64906-L64940).
- `unwrap_uploaded_json` — port of [frontend/core/export-format.js:26-34](../../frontend/core/export-format.js#L26-L34) (format-v2
  wrapper detection).
- `build_blocks` — port of [frontend/core/blocks.js:38-67](../../frontend/core/blocks.js#L38-L67), **UTC in, UTC out**;
  day-bucketing deliberately excluded (see §4.3).
- ALL-CAPS dictionary check — port of the §5.1 logic in
  [timeline-project-decisions.md:222-233](../../timeline-project-decisions.md#L222).
- Criticism keyword regex — port of the wide-net phrase list,
  [timeline-project-decisions.md:242-243](../../timeline-project-decisions.md#L242).
- Anger detection — **re-implemented against VADER** instead of AFINN
  ([timeline.html:64860 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L64860) is the AFINN table being replaced), keeping the
  existing anger-specific phrase list and exclamation-mark-burst logic
  ([timeline-project-decisions.md:246-249](../../timeline-project-decisions.md#L246)), with thresholds
  recalibrated against the same hand-curated baseline the AFINN version was tuned against.
- `effective_flag` four-state matrix — port of
  [frontend/core/flags.js:12-25](../../frontend/core/flags.js#L12-L25), for server-side Analytics aggregates.

**Stays client-side**: all rendering (SVG charts, markdown-lite renderer, tab UI). No network
calls exist yet — this version is a tested library, not a deployed service.

**AWS resources**: none.

**Tests**:
- Unit tests: the 6 hand-built dedup edge cases named in
  [timeline-project-decisions.md:66-70](../../timeline-project-decisions.md#L66) (simple chains, a stray
  reply to an early attempt, no duplicates, mid-conversation duplicates, a 5-way chain with a stray
  reply), ported as literal Rust cases.
- `proptest` property test: for any generated human/assistant message sequence, deduped output
  never has two adjacent-in-the-human-subsequence identical messages, and message count only
  decreases.
- Regression test against a real sample dataset, now checked in at
  [backend/timeline-core/tests/fixtures/sample_conversations.json](../../backend/timeline-core/tests/fixtures/sample_conversations.json)
  (see C1 for provenance and a discrepancy against the decisions doc's original stat that this
  surfaced): 6 conversations, 36 raw messages, **6 dropped by dedup, 30 remain** — 3 of the 6
  conversations (`IRS TIN match failure on sam.gov`, and windowed excerpts of `Starting a
  government contracting business` and `401k benefit administration for small businesses`) each
  contain one real resend-after-empty-assistant-reply duplicate pair, which is exactly the
  real-world case the dedup logic exists for.
- ALL-CAPS dictionary test: `IRS`/`DARPA`/`ICHRA`/`QSEHRA`/`OK` → zero matches;
  `WRONG`/`RIDICULOUS` → flagged ([timeline-project-decisions.md:232-233](../../timeline-project-decisions.md#L232)).
- VADER recalibration test: run the new anger detector against the same hand-curated
  criticism/anger baseline the AFINN version was checked against
  ([timeline-project-decisions.md:243-249](../../timeline-project-decisions.md#L243)); recall should not
  regress below the AFINN-based baseline's recall.
- `insta` snapshot test on `build_blocks` for a synthetic multi-day, multi-gap sequence, asserting
  session boundaries land exactly on the ≥15-minute gap rule.

### V2 — Deployed backend: storage, auth, upload/review flow (no Bedrock, no payment)
**Adds**: Lambda handlers (`axum` + `lambda_http`) behind API Gateway: `POST /uploads`
(presigned-URL issuance), the S3-triggered processing Lambda, `GET /conversations`,
`GET`/`PATCH /messages/{id}/flags` (two-field auto/user write path, structurally enforced — §4.1),
`GET /export`. Cognito gates all routes. `timeline.html` is refactored to `fetch()` these
endpoints instead of parsing a local file
([frontend/core/export-format.js:36 onward](../../frontend/core/export-format.js#L36) call sites removed); `window.storage`
auto-save/recovery ([timeline.html:65298-65365 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65298-L65365)) is retired — it was only
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

### V2a — Container-free local dev harness (added after V2's backend landed with no frontend wiring)

**Why this exists**: after V2's backend crates landed, `timeline.html` was still 100% unwired to
them — genuinely testable-in-a-browser was never actually reached, which is the whole point of
V2. Closing that gap exposed a second issue: the presigned-URL upload design is inherently
S3-shaped (the browser `PUT`s bytes directly to a signed HTTPS URL, and a real S3 event then
triggers a separate processing Lambda) — neither half of that has a meaning for the in-memory
adapters as written, so there was no way for a real browser to drive the in-memory backend
end-to-end at all, container or not. This section is the fix: a design that keeps every piece of
*logic* container-free and testable via the in-memory adapters, and confines any actual AWS/
container dependency to deployment-time verification of the adapters themselves — never to the
routine "does the website work" loop.

**The pure logic needs no new code.** `unwrap_uploaded_json`, `dedup_chat_messages`/
`dedup_conversations`, `build_blocks`, and the per-message heuristics (`has_emphasis_caps`,
`detect_critical`, `detect_angry`) already exist in `timeline-core` and are already 100%-covered
by public-API tests from V1 — see [§3 V1](#v1--rust-core-logic-no-aws-yet-pure-port--parity-proof)
above. What V2 never added was the thin orchestration that calls them and writes the results
through the storage ports.

**`process_upload(user_id, upload_id)`** (new, `timeline-api`): reads the raw bytes via
`ObjectStore::get`, calls the existing parse/dedup/heuristic functions in sequence, writes one
`ConversationSummary` per conversation via `ConversationStore` and one `FlagSet` per message via
`AutoFlagWriter`, then marks the `UploadRecord` `Ready` (or `Failed` on a parse error). It depends
only on the port traits, so it's testable against the in-memory fakes with no container and no
AWS — the same pattern already used for every other V2 handler test.

**Two callers of that one function**:
- **Production**: a separate Lambda, triggered by a real S3 `ObjectCreated` event, using the real
  `timeline-storage` S3/DynamoDB adapters — this is the only path that ever touches AWS or
  LocalStack, and only to verify the *trigger wiring* and the *real adapters*, which is deployment
  verification, not routine testing (the orchestration logic itself is already proven above).
- **Local dev**: a `_dev`-namespaced axum route on the same `timeline-api` binary, called directly
  after bytes land in the in-memory `ObjectStore` — no event system involved, just a direct call.

**Local-only HTTP endpoints, standing in for S3-specific mechanisms** (never present when running
against the real AWS adapters; clearly namespaced `_dev/...`, same spirit as
`dev_only_test_jwks.json`):
- `PUT /_dev/local-storage/{key}` / `GET /_dev/local-storage/{key}` — what `InMemoryObjectStore`'s
  `presign_put`/`presign_get` return instead of the current placeholder `memory://...` string,
  since a real browser needs an actual URL it can `PUT`/`GET` against. The `PUT` handler stores the
  bytes, then calls `process_upload` directly — the local substitute for the S3-event trigger.
- `POST /_dev/login` — mints a bearer token from the same throwaway RSA keypair already checked in
  for `timeline-auth`/`timeline-api` tests, since there is no real Cognito pool to log in against
  locally. Takes just a display name/sub, no password — it is not an auth mechanism, it's a stand-
  in for one, and must never exist in the Lambda/production build.

**`GET /export`** follows the same shape the plan already specifies (§1, "Annotated export... S3,
generated on demand... served back via presigned GET"): read conversations + flags via the
existing ports, serialize the annotated export JSON, `ObjectStore::put` it, return a presigned GET
URL — against the in-memory adapter, that URL is a `/_dev/local-storage/...` link like uploads
use. No new algorithmic logic, no container.

**`timeline.html`**: the upload/load code (currently local `FileReader`-based, plan §V2 already
calls for this to become `fetch()`-based) is rewritten to `POST /uploads`, `PUT` the file to the
returned `upload_url`, poll until the upload's status is `Ready`, then `fetch()` `/conversations`
and message flags — plus a minimal dev-only login screen that calls `POST /_dev/login` to obtain a
token in local testing. All of this only needs `cargo run -p timeline-api`; no container, no AWS.

**Tests**: `process_upload` gets black-box coverage the same way every other V2 handler does —
public-API tests against the in-memory fakes (a real upload → real processed conversations → real
flags, through the actual port methods, not by inspection). The `_dev/*` endpoints get their own
`tower::ServiceExt::oneshot` tests, same style as `timeline-api/tests/app.rs`.

**Status as of this section landing**: the backend half above is built, tested, and committed
(`ConversationStore::create`, `process_upload`, the `_dev`-only routes, `GET /export`). The
`timeline.html` half is not started. The rest of this section is the concrete plan for that
remaining piece, written and committed *before* touching `timeline.html`, per this repo's
Workflow rule in [CLAUDE.md](../../CLAUDE.md) (plan → user approval → code).

#### Implementation plan: wiring `timeline.html` (read-path increment)

**Scope of this increment**: upload a file through the real backend and see it rendered — the
minimum slice that makes "test the website in a browser" literally true. Deliberately not in
scope (see "Explicitly deferred" below): persisting the review table's flag overrides back to the
backend, and retiring the already-inert `window.storage` calls.

**Why no rewrite of the parsing/rendering code is needed**: `GET /export` already returns exactly
the `{"conversations": [...]}` wrapped shape `parseUploadedConversations`
([frontend/core/export-format.js:36](../../frontend/core/export-format.js#L36)) already knows how to consume — flags embedded as
`_claude_timeline_auto`/`_claude_timeline_user`, exactly the fields it already reads. So the only
thing that changes is *how the raw text reaches `parseUploadedConversations`*, not what happens to
it afterward. `CONVERSATIONS`/`MESSAGES`/`HUMAN_MESSAGES`/`BLOCKS` and every rendering function
stay untouched.

**New `handleLoadClick()` flow** ([frontend/ui/load-flow.js:141](../../frontend/ui/load-flow.js#L141) onward):
1. Read the chosen file's raw bytes client-side (same as today — needed to `PUT` them).
2. If no auth token is cached yet, call `POST /_dev/login` with a display name typed into one new
   text input (`id="devLoginSub"`, placed next to the existing file picker at
   [timeline.html:709-710](../../timeline.html#L709-L710)) to get one. Dev-only, matching the rest of
   this section.
3. `POST /uploads` (`Authorization: Bearer <token>`) → `{upload_id, upload_url}`.
4. `PUT` the raw bytes to `upload_url`.
5. `GET /export` (`Authorization: Bearer <token>`) → `{export_url}`, then `GET export_url` → the
   annotated JSON text.
   - **No polling needed for this increment**: the local-dev `_dev/local-storage` `PUT` handler
     (`timeline-api/src/routes/dev_local_storage.rs`) runs `process_upload` synchronously before
     its response returns, so by the time step 4 resolves, processing has already finished. This
     is a simplification specific to the local-dev trigger, not a general guarantee — real
     S3-event-triggered processing is asynchronous, so a production version needs an upload-status
     endpoint to poll first (not built; noted as a gap, not solved here).
6. Feed that text into the existing, unmodified `parseUploadedConversations(text, runAutoDetect,
   refreshAutoDetect)`. Everything downstream is unchanged.

**Also retired in this increment** (moved out of "deferred" after review — see below): the three
`window.storage`-backed load-time recovery calls in `handleLoadClick()`
([timeline.html:65086 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65086) and
[timeline.html:65102 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65102)) and one save call in `setRowOverrides()`
([timeline.html:65392 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65392)):
- `loadAutoClassificationsFromStorage()` and the recovery-merge block that follows it — removed
  outright, not just left uncalled. Merging cached LLM-classification results from an unrelated
  previous run on top of freshly-fetched backend data is exactly the kind of thing that could
  silently paper over a broken or incomplete server-side pipeline: if `process_upload` had a bug
  and produced wrong flags, stale cached data could make the UI look correct anyway.
- `loadOverrides()` and its call — same risk, for user overrides instead of auto flags.
- `saveOverrides()`'s call in `setRowOverrides()` — replaced with an honest
  `setSaveStatus('Not yet saved to the server — coming in a later increment.')`, since silently
  doing nothing while implying (via the old status text) that a save mechanism exists would be its
  own small dishonesty.

`saveAutoClassificationsToStorage()` (used only inside `classifyWithAI`'s batch loop, lines
65592-65612) is explicitly **not** touched — that function is gated to only run "while running as
a rendered Claude artifact" per its own comment, a distinct, still-legitimate feature this
increment doesn't build or exercise, and it can't mask anything about the upload/view flow above
(it's a checkpoint for a different, not-yet-backend-ported code path, not something that runs
during a normal load).

**Backend addition needed**: a permissive CORS layer (`tower_http::cors::CorsLayer`, MIT license,
same `tower` family already used) on the local-dev merged router only (`main.rs`'s local branch),
since `timeline.html` isn't served by `timeline-api` and will be opened separately (e.g. as a
local file or via a static server on a different port) — without it the browser blocks the
cross-origin `fetch()` calls. `tower_http` is a new dependency.

**Base URL**: a `const API_BASE = 'http://127.0.0.1:3000'` JS constant, not a relative path, since
`timeline.html` isn't guaranteed to be served from the API's own origin. Every new `fetch()` call
in this increment uses `${API_BASE}/...`.

**Persistence — spelled out explicitly, not left implicit**: none of this is durable. Every store
behind `timeline-api` in this increment (`InMemoryObjectStore`, `InMemoryUploadStore`,
`InMemoryConversationStore`, `InMemoryMessageFlagsStore`) is an `Arc<Mutex<HashMap>>` living
inside the `timeline-api` process. **Restarting `cargo run -p timeline-api` deletes every upload,
conversation summary, and flag.** This is the deliberate consequence of staying container-free and
AWS-free for routine testing (see C10 above for the real path to durable local storage, explicitly
out of scope here) — not a bug, but a fact worth stating plainly rather than letting "wired to a
backend" imply persistence it doesn't have.

**Also in scope for this increment (moved back in after review): wiring the review table's flag
overrides to the real backend.** Originally deferred with the justification "not a regression
since overrides already don't persist outside the artifact context" — that's true but was a weak
reason to skip real, readily-achievable functionality, since without it the "testable website"
never actually completes its core workflow (review a message, confirm a flag, have it stick for
the session). The actual complexity is low:
- `setRowOverrides(id, changedType, changedValue)` ([frontend/ui/flag-edits.js:28-39](../../frontend/ui/flag-edits.js#L28-L39))
  already computes the full three-flag `values` object and updates local `OVERRIDES`/re-renders
  optimistically, exactly as today.
- The real `(conversation_id, message_id)` UUIDs the backend needs are already available in
  existing client state: `RAW_DATA[msg.conv].uuid` and
  `RAW_DATA[msg.conv].chat_messages[msg.rawIndex].uuid` (`rawIndex` is already stored on every
  `HUMAN_MESSAGES` entry, from `parseUploadedConversations`).
- After the existing optimistic local update, fire `PATCH
  ${API_BASE}/conversations/{conversation_id}/messages/{message_id}/flags` with a
  `FlagOverrides`-shaped body (only the changed field set, others omitted — matching what
  `UserFlagWriter::set_user_flags` already expects). `saveOverrides()`'s call
  ([timeline.html:65392 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65392)) is replaced by this, not left calling
  `window.storage`.
- On a failed `PATCH`: surface it via `setSaveStatus(...)` (e.g. "Could not save — check your
  connection and try again"), matching the existing status-message pattern. The optimistic local
  update stays (so the UI doesn't flicker back), but the failure is visible, not silent.

**Still genuinely deferred** (blocked by lack of AWS access, not a scoping choice): real
(non-dev-only) login, upload-status polling for async S3-triggered processing, and anything
needing LocalStack or real AWS — see C10 above.

**Testing**: this is a browser-facing UI change, verified with a real, **committed, repeatable**
Playwright test — not a throwaway script. New top-level `e2e/` directory (`e2e/package.json`,
`@playwright/test` as a dependency — MIT license, the standard way to write this rather than a
bespoke script), driving the machine's already-installed Chrome
(`executablePath: '/usr/bin/google-chrome'`, `--no-sandbox` — no `sudo`, no bundled-Chromium
download needed; confirmed working directly before committing to this approach, see the earlier
smoke test). The test: starts `cargo run -p timeline-api` in the background (polling the port,
not sleeping), opens `timeline.html` in the driven browser, selects the test fixture via the real
file input, clicks the load button, and asserts against the rendered DOM (conversation list
populated, review table shows the expected flags) — not a hand-wave "should work," an actual
driven session with real assertions. `e2e/README.md` documents how to run it
(`npm test` inside `e2e/`) and that it needs Node ≥20 (installed via `nvm`, not the distro's
apt package, which was too old) plus the system Chrome already on this machine. Not wired into
`cargo test --workspace` (different toolchain entirely), but documented as a required manual step
before calling this increment done, the same way the plan already treats LocalStack/real-AWS
verification for other pieces.

### V2a-revision: what each store actually persists, and why — a design review found real problems

A design-review conversation after V2a landed surfaced that the storage ports' shapes were only
explicable by reciting AWS-specific constraints (DynamoDB's 400KB item cap, Lambda's
statelessness between invocations) — which is itself evidence the ports weren't actually hiding
infrastructure from `timeline-core` the way the crate's own stated principle requires. This
section is the write-up of that review and the resulting design, **not yet applied to code** —
per this repo's Workflow rule, plan first, approval, then code.

#### The one fact that actually justifies durable storage here

**A Lambda deployment gives no guarantee that two HTTP calls — even from the same user, seconds
apart — run in the same process.** Each invocation may reuse a "warm" execution environment from
an earlier one or start a fresh one; which happens is decided by AWS's internal scheduling,
invisible to the application, and explicitly not something AWS's own documentation says to rely
on. `POST /uploads`, the browser's direct `PUT` to S3, the S3-event-triggered processing Lambda,
and a later `GET /export` are four separate invocations with no shared memory to assume. That's
the actual reason any durable storage is needed at all — not "does it need to survive a server
restart," which was the framing this plan used until now, and which is the wrong question for a
serverless deployment: there is no persistent process for in-memory state to survive *as*. If the
deployment target were instead one long-running process with sticky sessions, none of this would
be necessary. It isn't — Lambda was chosen for elastic, pay-per-use scaling (§1.1) — so it is.

This single fact should be stated in the ports' own documentation, in infrastructure-neutral
terms ("this data must be readable by a process other than the one that wrote it, at an
unpredictable later time"), rather than requiring a reader to already know Lambda's execution
model to understand why a port exists at all.

#### Duration is not the axis that distinguishes the stores — access pattern is

| Store | Duration needed | Actual reason for its own technology |
|---|---|---|
| `ObjectStore` | Indefinite — written once, read by any number of later, unrelated invocations | Cheap, durable, arbitrary-size blob storage; no query capability needed |
| `ConversationStore` (data now called `ConversationSummary`) | **Also indefinite** — not shorter than `ObjectStore`'s | Not about duration — about access pattern: cheap point-reads on small records, so listing conversations or checking a flag never means fetching and parsing a potentially 60MB blob |
| `UploadStatus` | Short — useful only until the client has seen the final outcome | The one place a smaller mechanism suffices — see below |

`ObjectStore` and `ConversationStore` were being discussed as if they persisted for different
lengths of time. They don't. They differ in *shape of access* (opaque blob vs. small indexed
record), not durability. Conversation-addressability's concrete payoff, stated plainly instead of
abstractly: without a per-conversation index, `GET /conversations` for a user with a long upload
history means fetching and parsing *every upload blob they've ever submitted* — potentially
hundreds of MB — just to render a name-and-count list. The index turns that into O(number of
conversations) instead of O(total historical upload bytes). Not hypothetical at this tool's
documented scale (§7: up to 60MB single uploads, ~2,200 messages/user).

#### Concrete simplification: drop what nothing needs

Checked against actual call sites (`grep`, not assumed) before proposing this:

1. **`UploadStatus::Pending`/`Processing` are never read by anything.** No `GET /uploads/{id}`
   route exists; `process_upload` doesn't branch on status either. They exist to support a future
   client-facing "still processing…" message — legitimate in the target async design, but nothing
   currently reads them, and the intermediate states don't need to be *persisted values* even once
   that endpoint exists: a client can poll "does a terminal outcome exist yet?" (absent = not
   done) without the pipeline ever writing a distinct `Processing` record.
2. **`raw_object_key` doesn't need to be stored at all.** It's `format!("raw/{user_id}/{upload_id}.json")`
   ([timeline-api/src/routes/uploads.rs:38](../../backend/timeline-api/src/routes/uploads.rs#L38))
   — a pure function of `(user_id, upload_id)`, computed once when issuing the presigned URL and
   currently *also* round-tripped through `UploadStore` so `export.rs` can read it back
   ([timeline-api/src/routes/export.rs:66](../../backend/timeline-api/src/routes/export.rs#L66))
   — when it could just recompute the same format string locally instead.
3. **`create_pending`/`mark_processing` have no reason to exist once (1) and (2) are dropped.**
   There is nothing left to "register" before processing starts — an S3 event notification is
   self-describing (bucket + key), and the key itself already encodes `user_id`/`upload_id` by
   convention (exactly how the local-dev `_dev/local-storage` PUT handler already parses it,
   [timeline-api/src/routes/dev_local_storage.rs](../../backend/timeline-api/src/routes/dev_local_storage.rs)).

**Proposed replacement shape** for `timeline-core/src/ports/uploads.rs`:

```rust
pub enum UploadOutcome {
    Ready { conversation_ids: Vec<ConversationId> },
    Failed { reason: String },
}

#[async_trait]
pub trait UploadOutcomeStore: Send + Sync {
    /// Written once, when processing finishes -- there is no earlier,
    /// pending write. `POST /uploads` never touches this store at all.
    async fn record_outcome(
        &self, user_id: &UserId, upload_id: UploadId, outcome: UploadOutcome,
    ) -> Result<(), StoreError>;

    /// `None` means "not finished yet" -- the only status a polling client
    /// needs, without a separate persisted `Pending`/`Processing` value.
    async fn get_outcome(
        &self, user_id: &UserId, upload_id: UploadId,
    ) -> Result<Option<UploadOutcome>, StoreError>;
}
```

This removes `UploadRecord`, the 4-variant `UploadStatus`, `create_pending`, and `mark_processing`
entirely — not a rename, a real reduction in what this port's contract promises to do.

#### Naming, consolidated with the rest of this session's findings

- `ConversationStore` → **`ConversationSummaryStore`**: it stores `ConversationSummary` records
  (name, message count, a foreign key) — never the conversation's actual messages, which are
  never persisted as a separate structured thing at all (only reconstructed on demand by
  re-parsing the raw upload blob). "Store" without qualification claims more than that.
- `UploadStore` → **`UploadOutcomeStore`**, per the simplified shape above.
- `ObjectStore` keeps its name and revised module doc (already applied,
  [timeline-core/src/ports/object_store.rs](../../backend/timeline-core/src/ports/object_store.rs))
  — it's the one port that actually stores content, so "Store" is accurate there.
- `ConversationStore::create` → `put` (already applied, uncommitted in the working tree as of
  this write-up) is compatible with this revision and doesn't need to change again — `put` still
  correctly names an upsert regardless of the trait's own rename.

#### What this doesn't solve, stated honestly

Even after this simplification, `timeline-core`'s ports still encode a real infrastructure
requirement (durable, cross-process-readable storage) in their existence — that's unavoidable,
because the application genuinely cannot function without it, and *that* is a legitimate
domain-level fact (how the data will be read, not which AWS service backs it). What this revision
removes is the *AWS-Lambda-specific* residue riding along with it (an unused status lifecycle, a
value that's actually just a naming convention) — not the fact that a port exists to request
durable, indexed storage at all. A single, unified "persistent conversation storage" port
(replacing `ConversationSummaryStore` + `ObjectStore`'s `raw`-prefix usage with one contract) was
considered and rejected for now: it would still need to expose *some* way to distinguish
cheap-indexed-record access from bulk-blob access, or every adapter would have to fake one side of
that distinction the way the in-memory `ObjectStore` fake already does for presigned URLs — this
doesn't remove the two-shape reality, only its visibility.

### V2b — Test the real storage adapters against local stand-ins (closes C10)

**Why this exists**: the S3 adapter ([timeline-storage/src/s3.rs](../../backend/timeline-storage/src/s3.rs))
and the two DynamoDB adapters
([conversations_table.rs](../../backend/timeline-storage/src/dynamo/conversations_table.rs),
[message_flags_table.rs](../../backend/timeline-storage/src/dynamo/message_flags_table.rs)) are
the only code between this app and real AWS storage, and none of it has ever run. `s3.rs`'s own
header says so. Every test so far runs against the in-memory fakes, so nothing checks that the
fakes behave like the real services. This increment runs the real adapter code against local
programs that speak the same protocols as S3 and DynamoDB, with no Docker and no AWS account.

**What this does not prove**: that real AWS behaves like the stand-ins. The one-time run against
real S3 and DynamoDB in [§V2's test list](#v2--deployed-backend-storage-auth-uploadreview-flow-no-bedrock-no-payment)
is still required before V2 is done. One known difference: against a local stand-in, the tests
must put the bucket name in the URL path (`http://127.0.0.1:port/bucket/key`). Real S3 normally
puts it in the host name (`bucket.s3.amazonaws.com/key`). So the address form our code will use in
production is only tested by that real-AWS run. Also out of scope: the S3-event trigger, Cognito,
and `sam deploy`.

#### DynamoDB: Amazon's "DynamoDB Local"

- **What it is**: a free program from AWS that behaves like DynamoDB on your machine. It's a
  Java program, so it needs Java 17 or newer (AWS's download page says so).
- **One-time setup, done by you** (it needs `sudo`): `sudo apt install openjdk-21-jre-headless`.
  `apt` on this machine offers version 21.0.12. Java is not installed today; I checked.
- **Fetching it**: a new script, `scripts/fetch-dynamodb-local.sh`, downloads AWS's
  `dynamodb_local_latest.tar.gz` and unpacks it into `backend/.tools/dynamodb-local/`. That folder
  is added to `.gitignore`. The script does nothing if the right version is already there. AWS's
  download address always serves the newest release, so the script checks the file against a
  checksum written into the script itself. It doesn't use the checksum AWS publishes next to the
  file, because that one changes with every release. The pinned checksum is `f80bcec4…c21163`,
  for version 3.3.1 (dated 2026-05-28 in its release notes). I downloaded it on 2026-09-30, and
  it matched AWS's published checksum. When AWS releases a new version, the script stops with a
  message, instead of silently testing against a version nobody chose.
- **Starting it from the tests**: a test-support module,
  `timeline-storage/tests/support/dynamodb_local.rs`, starts
  `java -Djava.library.path=… -jar DynamoDBLocal.jar -inMemory -disableTelemetry -port <free port>`. `-disableTelemetry`
  is needed because DynamoDB Local sends usage data by default (see C15). It waits until
  a `ListTables` call succeeds, checking repeatedly with a 10-second limit rather than sleeping
  for a fixed time. It stops the program when the test binary exits. Each test creates its own
  tables under unique names, so tests can't see each other's data.
- **Table shapes**: tests create tables with the same key layout as
  [infra/template.yaml](../../infra/template.yaml) (`pk` as partition key and `sk` as sort key,
  both strings). The layout is written in one test helper. The risk that it drifts from the
  template is tracked as C16.
- **If Java or the JAR is missing, the tests fail. They are not skipped.** The failure message
  names the setup command and the fetch script. Skipping quietly would bring back the exact
  situation that let these adapters go untested for three weeks. The cost is that
  `cargo test --workspace` needs the one-time setup on any new machine. The alternative, putting
  these tests behind a Cargo feature (a switch you turn on at build time) that is off by default,
  is recorded in C17.
- **License**: DynamoDB Local is under AWS's own license, not an open-source one. I read it on
  2026-09-30. The plan's use fits its terms: a separate program on our own machines, testing code
  that will run against AWS, never checked in or shipped. It does require an AWS account in good
  standing, which you accepted on 2026-10-01. Details are in C15. Rules that follow from the
  license:
  - The JAR is never committed, attached to a release, or baked into an image we publish.
  - Tests run only on machines the account holder owns or controls. A hosted CI service counts
    only if the account holder controls the runner.
  - Nothing beyond its documented command-line options is used. No decompiling, no patching.
  - If the license ends, every copy is deleted, including caches and the `backend/.tools/` folder.

#### S3: stand-in choice (decided 2026-09-30: A, `s3s` + `s3s-fs`)

What our code needs from an S3 stand-in, taken from `s3.rs`: `PutObject`, `GetObject` (including a
missing key reported as `ObjectStoreError::NotFound`), and **presigned PUT and GET URLs**. A
presigned URL is a web address with a time-limited signature built in, so a browser can upload or
download one file without holding AWS keys. Presigning is the adapter's riskiest code, because a
wrong signature only shows up when the server checks it. So a stand-in that doesn't check
signatures can't catch the most likely bug.

I checked each candidate's license, activity and latest release on 2026-09-30, using the
GitHub API, crates.io, PyPI and each project's own documentation.

| Option | License | Activity | Runs how | Checks presigned signatures? |
|---|---|---|---|---|
| **A. `s3s` + `s3s-fs`** (Rust crates) | Apache-2.0 | v0.17.0, released 2026-09-24 | Inside the test process. No separate program | Yes, when a login check is turned on (`set_auth`). The crate has its own tests for presigned URLs |
| **B. Moto server** (Python) | Apache-2.0 | v5.2.3. Very widely used (about 8,700 GitHub stars) | Separate program: `pip install moto[server]`, then `moto_server` | Not confirmed. Its docs don't say, and I didn't find the answer |
| **C. VersityGW** (Go) | Apache-2.0 | v1.8.0, released 2026-09-04 | Separate program: one downloaded file, storing to a local folder | Its source has presigned-URL checking code (`presign-auth-reader.go`). I haven't run it |
| **D. Adobe S3Mock** (Java) | Apache-2.0 | v5.2.3, released 2026-09-19 | Separate Java program (Docker is recommended) | **No.** Its README says presigned URLs are "accepted but not validated" |

- **A. `s3s` + `s3s-fs`.**
  - *For:* It runs inside `cargo test`, with no install, extra program or port to manage. It
    checks presigned signatures, so a wrong signature fails the test. It stores files in a
    temporary folder. Its minimum Rust version is 1.96, and we have 1.98.1.
  - *Against:* `s3s-fs` calls itself "experimental" on crates.io. It's below version 1.0, so
    upgrades may change its API (the functions our tests call). It's a small project (about 310
    stars). Its maintainers warn it is "not a complete security boundary"; that doesn't matter
    for tests. It adds build time for the test build only (an HTTP server library, among others).
- **B. Moto server.**
  - *For:* It's the most widely used AWS fake. It could later stand in for other AWS services as
    well. Python 3.12 is already installed.
  - *Against:* It's a second program the tests must start and stop, and a Python dependency in a
    Rust project. I don't know whether it checks presigned signatures. Its docs warn that only
    `localhost` works, not `127.0.0.1`.
  - *My view:* recommended only if you'd rather use the most widely adopted tool.
- **C. VersityGW.**
  - *For:* It's a real S3 gateway meant for production, so it's likely the closest to real S3
    behavior. It ships as a single downloaded file.
  - *Against:* It's a separate program with the most setup of the four (access keys, a folder for
    its account data, a storage folder). It's built to be a server, not a test fake.
- **D. Adobe S3Mock.**
  - *For:* It would reuse the Java we install for DynamoDB Local.
  - *Against:* It doesn't check presigned signatures, which is the main thing we need to test. Its
    README recommends running it in Docker.

**Ruled out**:
- MinIO: its license is AGPL-3.0, which the project's license rule forbids, and its GitHub
  repository is archived.
- `s3rver`: archived in August 2025. Its last release was in 2021.
- LocalStack: its GitHub repository was archived in March 2026, and it needs Docker.
- Scality CloudServer: needs Node 24 or newer. This machine has Node 18.
- RustFS: only preview releases so far.
- SeaweedFS: a whole distributed storage system, far more than a test needs.

**Decision: A**, chosen by you on 2026-09-30. It is the only option that both checks presigned
signatures and adds nothing to install.

#### Tests

One **contract suite** per port: a set of tests describing what any correct implementation must
do, written once and run against both the in-memory fake and the real adapter. This is what
catches a fake that has drifted from the real service. The existing `memory_*.rs` tests in
[timeline-storage/tests/](../../backend/timeline-storage/tests/) stay untouched, because committed
tests aren't modified without your approval. Once the contract suites cover the same behavior,
I'll say which of the old tests are redundant.

- **`ObjectStore`** (fake and `S3ObjectStore`):
  - `put` then `get` returns the same bytes.
  - `get` of a missing key returns `NotFound`, not `Backend`.
  - For the S3 adapter: `presign_put`, then an HTTP `PUT` to that URL with a plain HTTP client,
    then `get`, returns the bytes. Likewise `put`, then `presign_get`, then an HTTP `GET`.
  - For the S3 adapter: a presigned URL with one character of its signature changed is rejected.
    Also, an expired URL is rejected. This proves the stand-in really checks signatures, so the
    passing presign tests mean something.
- **`UploadOutcomeStore` and `ConversationSummaryStore`** (fakes and `DynamoConversationsTable`):
  - Outcome and summary round-trips.
  - `get` of a missing item returns `NotFound`.
  - `list_for_user` returns only that user's summaries.
- **`MessageFlagsReader`, `AutoFlagWriter` and `UserFlagWriter`** (fake and
  `DynamoMessageFlagsStore`):
  - Automatic and user flags round-trip.
  - Writing automatic flags never changes user flags, and the reverse (the two-field rule, §4.1),
    checked by reading back what is stored rather than by inspecting the query text.
  - `list_for_conversation` returns only that conversation's flags.
- **Coverage**: `s3.rs` and both DynamoDB files reach 100% line coverage through these public-API
  tests. If any line can't be reached, I'll report it as a finding. I won't add a test that
  reaches into private code to force it.

**New test-only dependencies** (all under licenses the project allows):
- `s3s` and `s3s-fs` (Apache-2.0).
- `hyper-util` (MIT), to serve `s3s` on a local port.
- `reqwest` (MIT/Apache-2.0), to send the plain HTTP requests to presigned URLs.
- `tempfile` (MIT/Apache-2.0), for the stand-in's storage folder.

The exact versions get pinned when the code is written.

**Done means**:
- `cargo test --workspace` passes, including the new suites.
- `cargo llvm-cov` shows the numbers above.
- [backend/README.md](../../backend/README.md) documents the one-time setup.
- The "zero test coverage" header in `s3.rs` is replaced with what is now tested and what isn't.
- C10 is marked resolved, with links to these tests.

### V2c — Stored data and flag saves: no silent defaults, no unchecked IDs

**Status:** done 2026-10-01. Commits `37db425`, `1e66325` (malformed values); `85e00f0`,
`3b82e18` (empty saves); `5e4c314`, `250c4b7`, `3b3aaf3`, `ad32f8e`, `988a69a`, `f78ad35`,
`9ef9a5d` (flag handles). All suites pass: 293 Rust, 42 frontend unit, 59 browser. Line coverage:
`attributes.rs`, `conversations_table.rs` and `message_flags_table.rs` are at 100% apart from the
four commented backstops; `flag_handles.rs` and `routes/flags.rs` are at 100%.

**Where the implementation differs from this section:**
- The `/export` test checks that no handle appears anywhere in the exported file, and the existing
  export tests that check the file's contents pass unchanged. It does not compare the file byte for
  byte with its pre-change output; doing that would need the old code running alongside the new.
- The Lambda key is tested through `FlagHandleKey::from_env_value`, the function the Lambda calls
  at startup. Starting the Lambda process itself without the key isn't tested.
- More malformed-value tests than planned: 14 for conversation and upload rows (also covering
  wrong types and a negative count), 6 for flags.
- One extra backstop comment, in `attributes.rs`, for labelling a row that has no sort key.


**Why this exists**: V2b's review found that the two DynamoDB adapters fill in a default whenever a
value in a stored row is missing or has the wrong type, instead of reporting it. That hides
corrupted or mis-written data and breaks the "no silent swallows" rule in
[CLAUDE.md](../../CLAUDE.md). On 2026-10-01 the user asked for every one of these to be checked.

**How a row is stored.** Every DynamoDB row has a partition key, `pk` (which group it belongs to),
and a sort key, `sk` (which row it is within that group). The test tables, like the SAM template,
are defined so that DynamoDB refuses to store a row without both. The other attributes
(`status`, `name`, `auto_caps` and so on) are optional as far as DynamoDB is concerned. Our code
decides what they mean.

#### Every place a value is currently filled in or dropped

In [conversations_table.rs](../../backend/timeline-storage/src/dynamo/conversations_table.rs):

| Line | Attribute | Today, if missing or wrong type | Change to |
|---|---|---|---|
| 66–75 | `conversation_ids` on a `ready` upload row | missing: empty list | error |
| 70 | an entry in `conversation_ids` that isn't a string | dropped silently | error |
| 71 | an entry that isn't a valid id | dropped silently | error |
| 81–85 | `failure_reason` on a `failed` upload row | missing: empty string | error |
| 162–166 | `name` on a conversation row | missing: empty string | error |
| 167–171 | `message_count` on a conversation row | missing or not a whole number: 0 | error |

Every row of these kinds is written by our own code, which always writes every one of these
attributes. A `ready` row with no conversations is written as an empty list, not left out. So a
missing or malformed value means the row was corrupted or written by something else, and
reporting it is correct.

In [message_flags_table.rs](../../backend/timeline-storage/src/dynamo/message_flags_table.rs),
lines 117–124:

| Attribute | Missing today | Wrong type today | Change to |
|---|---|---|---|
| `auto_caps`, `auto_critical`, `auto_angry` | `false` | `false` | missing: keep `false`; wrong type: error |
| `user_caps`, `user_critical`, `user_angry` | no override | no override | missing: keep "no override"; wrong type: error |

Here "missing" is legitimate and must stay as it is. A user's override can create a row before any
automatic detection has run, so the `auto_*` attributes are absent; and `user_*` attributes are
absent until the user changes that flag. The in-memory stand-in treats both cases the same way.
Only a value of the wrong type, for example `auto_caps` stored as text, signals corruption.

#### The sort-key checks no test can reach

Three error checks guard situations that DynamoDB itself prevents:

1. [conversations_table.rs:207](../../backend/timeline-storage/src/dynamo/conversations_table.rs#L207),
   in `list_for_user`: "this row has no `sk`". DynamoDB won't store a row without `sk`, so it can
   never return one.
2. [conversations_table.rs:210](../../backend/timeline-storage/src/dynamo/conversations_table.rs#L210),
   also in `list_for_user`: "this row's `sk` doesn't start with `CONV#`". The query that fetches
   the rows asks DynamoDB only for rows whose `sk` starts with `CONV#`. So every row it returns
   has the prefix.
3. [message_flags_table.rs:156](../../backend/timeline-storage/src/dynamo/message_flags_table.rs#L156),
   in `list_for_conversation`: "this row has no `sk`". Same reason as 1.

**Decision (2026-10-01, the user):** these stay as they are, each with a comment saying it is
currently unreachable and kept as a backstop. Caught errors that current code can't throw, but
future code could, are an accepted exception to 100% coverage. The comments were added in the
same commit as this decision. The earlier proposal to route them through shared helpers is
dropped.

#### Design

One new module, `timeline-storage/src/dynamo/attributes.rs`, holds the reading rules for both
adapters:

- `required_string(item, name)`
- `required_count(item, name)`, a whole number of 0 or more
- `required_id_list(item, name)`, a list of valid ids
- `optional_bool(item, name)`: absent gives "not set", present but not true/false is an error

Each returns a distinct error message for "missing" and for "wrong type", naming the attribute,
the table row's `sk` and what was found, so an operator reading the logs can tell which row and
which attribute broke. They return `StoreError::Backend`, as the existing malformed-row errors
already do. This reuses the adapters' existing `invalid_data` error shape rather than adding a
new error type.

#### Tests

Public-API tests, in the style V2b already uses: write a raw row with the adapter bypassed, then
read it through the real trait method. One test per malformed case in the two tables above:
6 for conversation and upload rows, and 6 for the flag attributes stored with the wrong type
(12 in all). Each asserts a `Backend` error whose message names the attribute. The cases where
"missing" is legitimate are already covered by the contract tests
`a_user_write_on_a_message_with_no_record_creates_one_with_no_auto_flags` and
`an_auto_write_sets_no_user_override`, which must keep passing. All run against DynamoDB
Local. The existing contract suites must still pass unchanged.

#### Saving user flags: an empty request, and the two stores disagreeing

Added 2026-10-01, approved by the user (points 1 and 2 from that day's discussion).

**The disagreement.** `set_user_flags` takes up to three changes, each "set to true/false" or "leave
alone". When all three are "leave alone" and the message has no saved flag record yet, the stores
disagree:

| | In-memory stand-in | DynamoDB |
|---|---|---|
| Stores | a new blank record | nothing |
| Returns | that blank record (route answers 200) | "not found" (route answers 404) |

The page never sends such a request; it always sends all three flags as true or false
([flag-edits.js:27-37](../../frontend/ui/flag-edits.js#L27-L37)). Upload processing skips reviews
with no changes. Only a direct call to `PATCH .../flags` can send one: `{}`, all three `null`, or
misspelled field names, which the server currently ignores silently.

**Change 1: the route rejects bad requests.**
[routes/flags.rs](../../backend/timeline-api/src/routes/flags.rs) gets its own request type,
`FlagPatchRequest`, which refuses unknown field names (`#[serde(deny_unknown_fields)]`) and is
converted to `FlagOverrides`. A request with no changes, or with an unknown field, gets 400 Bad
Request with a message saying which. The shared `FlagOverrides` type stays as it is, because upload
processing also reads it from uploaded files, and making it strict there is a separate decision.

**Change 2: the in-memory stand-in matches DynamoDB.** An empty save on a message with no record
returns "not found" and creates nothing
([memory/message_flags.rs](../../backend/timeline-storage/src/memory/message_flags.rs)). A new
contract test, `an_empty_user_update_on_a_message_with_no_record_is_not_found_and_creates_nothing`,
pins this for both stores. It checks that a later `get` still returns nothing.

**Tests for change 1**, in `timeline-api/tests/`, sending real requests to the router the way
`app.rs` does: `{}`, all three `null`, and an unknown field each get 400. A valid request is still
accepted, which the existing PATCH tests already cover.

#### Flag handles: saves only for messages the server sent

Added 2026-10-01 at the user's direction.

**The gap.** Saving flags names a conversation ID and a message ID, and today the server stores the
flags without checking that such a message exists. Messages aren't stored individually; they exist
only inside the uploaded `conversations.json` in S3. Checking each save against that file would
mean downloading and parsing it, up to tens of MB, on every checkbox click. Orphaned rows are
invisible, because the export only looks up flags for messages in the file
([export.rs:85](../../backend/timeline-api/src/routes/export.rs#L85)), and they can only be written
under the caller's own account. But a buggy or malicious client could pile them up.

**The fix: a handle per message, issued by the server and required on every save.**

- **What a handle is.** A signature over the user ID, conversation ID and message ID, computed with
  a secret key only the server holds (HMAC-SHA256). On a save, the server recomputes it from the
  IDs in the request and compares. A made-up ID won't match, and nobody without the key can make a
  handle that does. Nothing is stored, and checking needs no database lookup. This works like the
  presigned S3 addresses tested in §V2b, which carry their own proof.
- **What it does and doesn't stop.** It stops saves for messages that don't exist in the user's
  uploads. It doesn't stop the user themselves from scripting saves, since they can read their own
  handles in the browser. That's acceptable: such a script can only do what clicking already does.
- **Exact signed content.** The fixed label `flag-handle-v1`, then the user ID, conversation ID and
  message ID, each preceded by its length in bytes. The length prefix means two different ID
  combinations can never produce the same signed text, even if a user ID contains a separator
  character. The handle is sent as unpadded base64url text, 43 characters. The comparison uses the
  `hmac` crate's constant-time check (`verify_slice`), so timing reveals nothing about a correct
  handle.
- **Where the page gets handles.** In the reply to `GET /export`, never in `conversations.json`,
  which is unchanged. The route already goes through every user message to build the file
  ([export.rs:85](../../backend/timeline-api/src/routes/export.rs#L85)); in that same pass it adds
  each message's handle to its reply: `{"export_url": "...", "flag_handles": {"<message id>":
  "<handle>"}}`. No extra download or parsing.
- **Where the page sends them.** [load-flow.js](../../frontend/ui/load-flow.js) keeps
  `flag_handles` in page state, from both places it calls `/export` (lines 79 and 190).
  `patchFlagsToBackend` in [api-client.js](../../frontend/infra/api-client.js) sends the message's
  handle in the request body. A message with no handle is reported with the existing "couldn't find
  this message's server-side id" outcome, not sent.
- **Checking on the server.** `FlagPatchRequest` (Change 1 above) gains a required `handle` field.
  The route verifies it before calling `set_user_flags`. A missing handle is 400 Bad Request; a
  handle that doesn't match is 403 Forbidden. [flag-edits.js](../../frontend/ui/flag-edits.js)
  words a 403 as "Could not save: this page's data is out of date. Reload the page."
- **Not affected.** Upload processing writes reviews read from the uploaded file itself, so it
  already knows the messages are real. Detection writes automatic flags, not user flags.
- **The key.** New module `timeline-api/src/flag_handles.rs` holds signing and checking.
  - Locally, the key is 32 random bytes generated at startup with `rand` (already a dependency),
    as the dev login keys already are ([dev_only.rs](../../backend/timeline-api/src/dev_only.rs)).
    Restarting the local server invalidates handles, but it also wipes all in-memory data, so the
    page has to reload either way.
  - On Lambda, the key comes from an environment variable that
    [infra/template.yaml](../../infra/template.yaml) fills from AWS Secrets Manager. If it's
    missing or shorter than 32 bytes, the Lambda refuses to start, with an error naming the
    variable. It never silently falls back to a generated key, because separate Lambda instances
    would then reject each other's handles.
- **Library.** `hmac` and `sha2` from RustCrypto (both MIT/Apache-2.0), already in the build
  indirectly through the AWS SDK's request signing; `base64` (MIT/Apache-2.0), likewise already
  present.

**Tests.**
- Route tests in `timeline-api/tests/`, sending real requests to the router: a valid handle saves;
  rejected with 403 are a handle for a different message, a different conversation, a different
  user, a made-up message ID with someone else's handle, and a handle with one character changed;
  rejected with 400 is a missing handle.
- An `/export` test: every user message in the file has a handle in the reply, and the file itself
  is byte-for-byte what it was before this change, so `conversations.json` provably doesn't change.
- The existing browser tests that click checkboxes and confirm the save keep passing; a new browser
  test confirms a 403 shows the "reload the page" message.
- A test that a Lambda configuration without the key variable fails to start.

**Done means**: all suites pass; the coverage report shows both DynamoDB files at 100% line
coverage apart from the three commented sort-key backstops, and `flag_handles.rs` at 100%; and C18
is marked resolved.

### V2d — The Lambda build uses the real AWS stores and real Cognito logins (closes C21)

**Status:** done 2026-10-01, commits `cc64e7c` (code) and `412fa1a` (tests). All suites pass: 303
Rust, 42 frontend unit, 60 browser. `aws_settings.rs` and `aws_state.rs` are at 100% line
coverage. Differences from the design below: the settings module is named `aws_settings.rs`, not
`aws_config.rs`, to avoid confusion with the `aws-config` library. Not tested: `main.rs` itself,
including the Lambda branch's wiring (it never runs inside the test suite); the tests cover the
functions it calls.

**What's wrong today.** [main.rs](../../backend/timeline-api/src/main.rs) decides at startup
whether it's running in Lambda (Lambda sets `AWS_LAMBDA_RUNTIME_API`). Both branches call the same
`build_local_state`, so a deployed Lambda would:
- keep every upload, summary and flag **in that one instance's memory**: lost when the instance
  stops, and invisible to any other instance serving the same user;
- check logins against the **throwaway dev key pair** generated at startup, not against your
  Cognito user pool. Real Cognito tokens would be refused, and only tokens signed by that
  instance's dev key would be accepted.

Only the flag-handle key differs between the branches (it comes from the environment since §V2c).

**Why it's like this.** The startup code was written on 2026-09-10 (commit `d7312c6`), when this
environment had no AWS access. Its own header comment still says local mode uses the in-memory
stores "because there is no AWS access in this environment to build real S3/DynamoDB clients
against". The Lambda branch was added so the binary could start under Lambda's runtime, but it
was never given its own state. The S3 and DynamoDB adapters it should use weren't tested until
§V2b. Nothing has been deployed, so no data or logins are affected.

**What this section doesn't cover.** Processing an upload on AWS needs a second Lambda, triggered
when the file lands in S3 (§V2's "S3-triggered processing Lambda"). It isn't built, and
[infra/template.yaml](../../infra/template.yaml) says so. Until it exists, uploads on AWS are
stored but never processed. It's the next piece after this one.

#### Design

1. **Read the configuration once, at startup, into typed values.** New module
   `timeline-api/src/aws_config.rs`:
   - `AwsSettings::from_lookup(lookup)` reads `TIMELINE_UPLOADS_BUCKET`,
     `TIMELINE_CONVERSATIONS_TABLE`, `TIMELINE_MESSAGE_FLAGS_TABLE`, `TIMELINE_COGNITO_USER_POOL_ID`,
     `TIMELINE_COGNITO_CLIENT_ID` and `AWS_REGION` (the template already sets all but the last,
     which Lambda sets itself).
   - Each becomes its own small type (`BucketName`, `TableName`, `UserPoolId`, `ClientId`,
     `Region`), so two names can't be swapped by accident.
   - A missing or empty variable is an error naming it. All problems are reported together, not
     one per restart.
   - `lookup` is a function argument (in production, `std::env::var`), so tests can supply values
     without changing the real environment.
2. **Build the real state.** New function `build_aws_state(settings, clients, jwks, flag_handle_key)`
   returns an `AppState` with `S3ObjectStore`, `DynamoConversationsTable`,
   `DynamoMessageFlagsStore`, and a `CognitoVerifier` using the user pool's issuer
   (`https://cognito-idp.<region>.amazonaws.com/<pool id>`) and client ID. The AWS clients are
   passed in rather than created inside, so tests can hand it clients pointed at the local
   stand-ins from §V2b; production creates them with `aws-config` (Apache-2.0, version 1.12 at the
   time of writing).
3. **Fetch Cognito's public keys at startup.** `fetch_jwks(region, pool)` downloads
   `https://cognito-idp.<region>.amazonaws.com/<pool id>/.well-known/jwks.json` with `reqwest`
   (MIT/Apache-2.0, already used by the storage tests). If the download fails, the Lambda refuses
   to start, with the address and error in the message. It never falls back to the dev keys.
4. **Wire it into the Lambda branch** of `main.rs`. The local branch doesn't change, and the
   header comment is corrected.
5. **Keep the dev keys out of the Lambda branch structurally.** `build_aws_state` doesn't depend
   on `dev_only` at all, and a test checks that a token signed with the dev key pair is refused
   by the router `build_aws_state` produces.

#### Tests

- **Configuration:** every variable missing (one at a time), empty, and all present; the error
  names every missing one at once.
- **The real router against local stand-ins.** This is the Lambda's exact router, built by
  `build_aws_state`, with S3 served by `s3s-fs`, DynamoDB by DynamoDB Local, and the Cognito keys
  by a small local HTTP server serving a test key set. Each test:
  - logs in with a token signed by the test key, with the right issuer and client ID;
  - writes a conversation summary and flags through the real adapters, as upload processing would;
  - reads them back through `GET /conversations`, `PATCH .../flags` (with a handle from
    `GET /export`) and `GET /export`;
  - checks a second router built from the same settings sees the same data, which is the
    "survives across instances" property the in-memory state lacks.
- **Refusals:** a token signed with the dev key pair, a token for the wrong client ID, and a token
  from the wrong issuer are each refused with 401. A key-set download that fails stops startup
  with the address in the message.
- **Not covered here:** running inside the real Lambda runtime, and real Cognito and real AWS.
  Those wait for the first deployment in §V2. `cargo lambda watch` (local Lambda runtime
  emulation, already used once and documented in [backend/README.md](../../backend/README.md))
  is a manual check after this section is built, not a gating test.

**Done means:** all suites pass; the new modules are at 100% line coverage; the Lambda branch of
`main.rs` no longer calls `build_local_state`; C21 is marked resolved.

### V2e — Make the first deployment usable, and test everything that can be tested locally

**Status:** built and tested on this machine 2026-10-01 (commits `bd44ee5` to `15b6254`); **not
deployed**, so V2 is not done until checks D1–D7 and D9 below are run. All suites pass: 325 Rust, 56
frontend unit, 65 browser, 8 deployment-script checks (`scripts/test-deploy-scripts.sh`). The new
and changed Rust modules are at 100% line coverage (`cargo llvm-cov -p timeline-api -p
timeline-core`). A first coverage run over the whole workspace was killed partway (exit 137);
the narrower run completed, and I haven't traced why the first one was stopped.
`scripts/check-template.sh` (`sam validate --lint`, SAM CLI 1.166.2) passes.

What running it showed, beyond the design below:
- **The named-stage 404 is observed, not just read**: the E1 test sending a request on stage `dev`
  gets 404 from the Lambda's router.
- **The circular dependency is observed too**: with the processing function's permission put back
  to `!Ref RawUploadsBucket`, `sam validate --lint` fails with cfn-lint's E3004 (circular
  dependency) across the bucket, the function, its role and its permission. The template as
  committed passes.
- **Processing measurement** (`scripts/measure-processing.sh`, 62.8 MB synthetic export, 161
  conversations): 188 MB peak memory both runs; 0.24 s, then 0.17 s on identical input (cause of
  the difference not traced). Template: 512 MB, 300 s.

Differences from the design below:
- **How the page finds a deployment's settings (E5).** The design had the page always ask for
  `frontend/deploy-config.json` and treat "missing" as local development. A missing file is a 404,
  which Chrome logs as a console error, and the browser tests fail any test that logs one. Instead,
  visiting `timeline.html?deploy=<name>` once (remembered, like `?api_base=`) loads
  `frontend/deploy-configs/<name>.json`; without it, no settings file is requested at all.
  `scripts/write-deploy-config.sh <stage>` writes the file; the folder's JSON files are ignored by
  git.
- **`FrontendOrigin` instead of `FrontendUrl` (E5, E6).** CORS needs the origin and Cognito the
  full address; the template takes the origin and builds `<origin>/timeline.html` from it.
- **The page's new pure logic is in `frontend/core/`** (`server-url.js`, `upload-wait.js`,
  `deploy-config.js`), not `frontend/infra/`: the structure test forbids `infra/` files importing
  each other, and these touch neither the network nor the browser.
- **Shared test setup:** `timeline-api/tests/support/aws_world.rs`, used by the new test files.
  `tests/aws_state.rs` keeps its own copy, unchanged.
- **Also added:** tests for the deployment scripts, against a stand-in `aws` command
  (`scripts/test-deploy-scripts.sh`); the ID token's lifetime set to one hour alongside the
  access token's.
- **D8 moved into C24 (2026-10-01).** It was a task to replace the tests' sample events, not a
  check that the deployment works: D3–D7 already run real events through the real code. You chose
  to capture the S3 notification only, during the first deployment; the API request stays the
  library's sample. Capturing needs a small code change, not yet chosen (C33).
- **Bucket-name copies are tested** (commit `8717329`, C34): every place the template writes out the
  uploads bucket's name must match the bucket's own.

**Why this exists.** A review on 2026-10-01 of what a first `sam deploy` would actually do found
that the deployed app couldn't be used even if every resource were created correctly:

1. **Uploads are never processed on AWS.** [processing.rs](../../backend/timeline-api/src/processing.rs)
   is built and tested, but nothing runs it when a file lands in S3. Its own module comment names
   `src/bin/process_upload.rs`, which doesn't exist.
2. **Every logged-in request would probably get 404.** The template names the API stage `dev`. A
   *stage* is a named version of an API that appears in its address. I read in `lambda_http`
   0.15.1 (`src/request.rs`, `apigw_path_with_stage`) that, unless the environment variable
   `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH` is set, a request for `/conversations` on stage `dev`
   reaches the router as `/dev/conversations`. The router only knows `/conversations`. Not run;
   read in source.
3. **The page can't reach the deployed API from a browser.** The API has no CORS settings (CORS:
   the browser rule deciding whether a page at one address may call a server at another). The
   S3 bucket allows every origin, which the template's own comment says to tighten.
4. **The page can't log in.** [api-client.js](../../frontend/infra/api-client.js) only knows
   `POST /_dev/login`, which the Lambda build never has.
5. **The page can't use S3's addresses.** The page builds every upload and download address as
   `API_BASE + url` ([load-flow.js](../../frontend/ui/load-flow.js)). That works for the local
   server's relative `/_dev/local-storage/...` paths, but S3's presigned addresses are already
   complete (`https://...`), so prefixing them breaks them.
6. **The page doesn't wait for processing.** Locally, the upload request processes the file
   before it answers, so the page asks for the export straight away. On AWS, processing starts
   only after the upload finishes and runs separately, so the page would ask too early. There is
   no route to ask whether an upload has finished: the `UploadOutcomeStore` port and its DynamoDB
   adapter exist, but no route reads them.
7. **There's no way to get a test login from the command line.** The login client only allows
   SRP, a challenge-and-response login the `aws` tool can't perform in one command.

**Why these weren't in an earlier stage.** V2a deferred the real login and processing as "blocked by
lack of AWS access" ([line 386](2026-09-09-rust-aws-backend-migration.md#L386)); that was true only of the last step of each, and nobody
revisited the list once V2b built local stand-ins. V2d left `main.rs`'s Lambda startup untested, and
the conversion from AWS's request format, where the stage name is added, sits in that gap. CORS was
designed for local use only (§V2a). No stage asked "what would stop a first deployment from working?"

**The rule for this section:** everything that can be tested on this machine is tested here, and
what only a deployment can check is listed at the end, so the first deployment confirms a known
list instead of finding surprises.

**Out of scope:** hosting `timeline.html` on AWS (the page is still served from this machine and
pointed at the deployed API; see C30), the `Users` table, payment (V4), Bedrock (V3).

#### E1. API addresses without a stage name

- Set the HTTP API's stage to `$default` in [infra/template.yaml](../../infra/template.yaml).
  `$default` is the stage that adds nothing to the address, so `/conversations` arrives as
  `/conversations` and `lambda_http` leaves it alone (the source above returns the path unchanged
  for `$default`). The `Stage` parameter keeps naming the resources (`timeline-api-dev`, ...); it
  just no longer appears in the API's address. Update the `ApiUrl` output to match.
- Chosen over setting `AWS_LAMBDA_HTTP_IGNORE_STAGE_IN_PATH`: that works through a process-wide
  environment variable, which tests can only exercise by changing the environment of the whole
  test process.
- **Tests** (new, `timeline-api/tests/lambda_events.rs`): feed AWS-format request events through
  `lambda_http::request::from_str` (public, `src/request.rs`) into the router `build_aws_state`
  builds, against the V2b stand-ins:
  - the sample HTTP API event with a JWT authorizer that ships with `aws_lambda_events` 0.16.1
    (`src/fixtures/example-apigw-v2-request-jwt-authorizer.json`, stage `$default`), with its path
    changed to `/conversations` and a valid test token added: answered by the real route;
  - the same event with stage `dev`: answered 404. This pins the reason for E1, so a later change
    back to a named stage fails a test instead of a deployment.
  - The sample events come from the library, not from our deployment; see C24.

#### E2. The upload-processing Lambda

- **One home for the key format.** `parse_raw_upload_key` is private in
  [dev_local_storage.rs:29](../../backend/timeline-api/src/routes/dev_local_storage.rs#L29); move it
  to `timeline_core::ports::uploads`, next to `raw_object_key`, which writes the same format. Both
  callers (the local upload route and the new Lambda) use it.
- **Settings the processing Lambda actually needs.** `AwsSettings` requires the Cognito pool and
  client, which the processing Lambda doesn't use. Add `StorageSettings` (bucket and the two
  tables) in [aws_settings.rs](../../backend/timeline-api/src/aws_settings.rs), read the same way
  (every missing variable reported at once). `AwsSettings` keeps its fields and reads the storage
  variables through the same reader. See C31.
- **The handler** (new, `timeline-api/src/s3_trigger.rs`): `handle_s3_event(event, deps)` takes an
  `S3Event` (from `aws_lambda_events`, MIT/Apache-2.0, already in the build through `lambda_http`;
  needs its `s3` feature) and the stores, and for each record:
  - decodes the key (S3 events encode keys the way web forms do, with `+` for spaces) and parses
    it into user and upload. A key that isn't a raw upload is an error naming the key (rule 3 of
    CLAUDE.md's exception handling);
  - calls `process_upload`. A file that isn't a valid export already records a `Failed` outcome
    there; the handler logs it and reports success, because retrying a bad file can't help;
  - a storage failure is returned as an error, so Lambda retries the event. AWS's documentation
    says an S3-triggered Lambda is retried twice by default; I haven't checked that against a real
    run. Retrying is safe because every write in `process_upload` replaces rather than adds
    (summaries `put`, reviews `set_user_flags`, outcome `record_outcome`); see C29.
  - Every record is attempted before an error is returned, and the error names each failed key.
- **The binary** (new, `timeline-api/src/bin/process_upload.rs`): reads `StorageSettings`,
  builds the S3 and DynamoDB clients as `main.rs` does, and runs `handle_s3_event` under
  `lambda_runtime`. Kept to wiring only, like `main.rs`.
- **Template:** a `ProcessUploadFunction` (code from
  `../backend/target/lambda/process_upload/`), triggered by `s3:ObjectCreated:*` on the uploads
  bucket, filtered to keys starting `raw/` and ending `.json`, so the exports the API writes under
  `export/` never trigger it. Permissions: read the bucket, read and write the two tables.
  - AWS's SAM documentation warns that a function triggered by a bucket, whose permissions also
    name that bucket with `!Ref`, makes a circular dependency the deployment refuses. The bucket
    name is already a fixed pattern (`timeline-uploads-${Stage}-${AWS::AccountId}`), so both
    functions' permissions and settings use that pattern through `!Sub` instead. Not reproduced;
    `sam validate --lint` (E8) and the first deployment check it.
  - Memory and time limit: set from a measurement, not a guess. A new script,
    `scripts/measure-processing.sh`, builds a ~60 MB synthetic export with the existing
    [e2e/synthetic-export.js](../../e2e/synthetic-export.js), runs `process_upload` on it in a
    release build against the in-memory stores, and reports peak memory (`/usr/bin/time -v`) and
    time. The template gets twice the measured memory, rounded up to a size Lambda offers, and a
    time limit of 5 minutes or three times the measured time, whichever is larger. The measured
    figures are recorded in this section. Lambda's speed scales with memory, so the measured time
    is a guide only; the real figure is a deployment check (D5).
- **Tests** (new, `timeline-api/tests/s3_trigger.rs`), against `s3s-fs` and DynamoDB Local as in
  V2b, using `aws_lambda_events`' sample S3 event (`src/fixtures/example-s3-event.json`) with its
  bucket and key replaced:
  - a valid upload: summaries and a `Ready` outcome are stored;
  - a file that isn't an export: a `Failed` outcome with the reason, and the handler succeeds;
  - a key outside `raw/`, and a key whose user or upload part doesn't parse: an error naming the key;
  - an encoded key (`%2D`, `+`) is decoded before parsing;
  - two records, one of them missing from S3: the other is processed and the error names the
    missing key;
  - processing the same event twice leaves the same stored data as processing it once.
  - `StorageSettings`: each variable missing, empty, and all present.

#### E3. Asking whether an upload has finished

- New route `GET /uploads/{upload_id}`: `{"status": "processing"}` while no outcome exists,
  `{"status": "ready"}` or `{"status": "failed", "reason": "..."}` once one does. It reads the
  outcome under the logged-in user's id, so nobody can see another user's upload.
- `AppState` gains `upload_outcome_store`. The API only ever reads it; the port has both methods
  in one trait. Splitting it into a reader and a writer, as the flag ports are split, isn't worth it
  for one route; recorded here as a choice, not an oversight.
- Adding a field to `AppState` means every test that builds an `AppState` by hand needs one more
  line. That changes committed tests; see C25.
- **Tests** (`timeline-api/tests/app.rs` style, in-memory stores, plus one against DynamoDB Local
  through `build_aws_state`): no outcome yet, ready, failed with reason, another user's upload
  (answered as "processing", same as one that doesn't exist, so it reveals nothing), not logged in
  (401), an upload id that isn't a UUID (400).

#### E4. The page: full addresses and waiting for processing

- New function `resolveUrl(url)` in [api-client.js](../../frontend/infra/api-client.js): an address
  that starts with `http://` or `https://` is used as it is; anything else gets `API_BASE` in
  front. Every upload, download and export address goes through it.
- After the upload, the page asks `GET /uploads/{id}` once a second for the first 10 seconds, then
  every 5 seconds, and stops after 10 minutes with a message saying processing didn't finish. A
  `failed` answer shows the server's reason. The same code runs locally, where the first answer is
  already `ready`.
- **Tests:** unit tests (`frontend/tests/`, `node --test`) for `resolveUrl` and for the waiting loop
  with a pretend `fetch` and clock: ready at once, ready after several answers, failed with reason,
  timing out, and a network error. The existing browser tests in `e2e/` must still pass unchanged.

#### E5. Logging in with Cognito

- **Cognito's own login page**, not a form in our page. The page sends you to Cognito's hosted
  login page, which handles sign-up, email confirmation and forgotten passwords, and sends you back
  with a one-time code that the page exchanges for tokens. The exchange uses PKCE, a standard way
  for a page with no server-side secret to prove it started the login. This keeps password handling
  out of our code, which is why §1.4 chose Cognito.
- **Library:** `oidc-client-ts` 3.5.0 (Apache-2.0; its one dependency, `jwt-decode`, is MIT), a
  copy of its prebuilt browser file kept in `frontend/vendor/`, since the page has no build step.
  Whether that file loads without a build step is unverified; see C32.
- **Template:** a Cognito domain (prefix `timeline-${Stage}-${AWS::AccountId}`); the app client gets
  the authorization-code flow, scopes `openid` and `email`, Cognito's own user directory as its
  only sign-in source, and a callback address from a new `FrontendUrl` parameter (default
  `http://localhost:8000/timeline.html`; Cognito accepts plain `http` only for `localhost`).
- **How the page knows which login to use:** a new script, `scripts/write-deploy-config.sh`, reads
  the deployed stack's outputs and writes `frontend/deploy-config.json` (API address, Cognito
  domain, client id). The file is ignored by git, as it describes one person's deployment. The
  page loads it at start: present means Cognito login against that API, absent means today's dev
  login against the local server. Nothing changes for local development.
- **Where tokens live:** `sessionStorage`, so a reload in the same tab stays logged in and closing
  the tab logs you out. Cognito's access token lasts one hour (template, `AccessTokenValidity`);
  after that the page sends you through the login page again. Restoring the last session on
  reload works the same way, using the stored token instead of the dev login name.
- **Tests:**
  - unit tests: choosing the login from the config file (present, absent, malformed: a malformed
    file is an error on the page, not a silent fall back to dev login);
  - a new browser test (`e2e/`) with a stand-in login server, a small Node server in `e2e/` that
    plays Cognito's part: its login address sends the browser straight back with a code, and its
    token address checks the PKCE proof and returns a token it got from the local backend's
    `POST /_dev/login`, so the local backend accepts it. The test runs the whole flow: page →
    stand-in login → back → upload → timeline shown, and checks the code no longer appears in the
    page's address afterwards. A wrong PKCE proof is refused and the page shows the error.

#### E6. Browser permissions (CORS)

- The `FrontendUrl` parameter's origin (scheme, host and port) becomes the only allowed origin, on
  both the API and the S3 bucket. The API allows the headers `authorization` and `content-type`
  and the methods `GET`, `POST` and `PATCH`.
- **The browser's permission check before the real request.** Browsers send an `OPTIONS` request
  first, without the login token. The template's single `ANY /{proxy+}` route matches `OPTIONS`
  too, so I expect that check to meet the login requirement and be refused. AWS's documentation
  says API Gateway answers `OPTIONS` itself when CORS is configured and no route matches it; so
  the template replaces `ANY` with separate `GET`, `POST` and `PATCH` routes. This reading of the
  documentation is unverified; see C26.
- **Tests:** none possible locally; API Gateway's handling exists only on AWS. Checked by
  `sam validate --lint` (E8) and deployment check D2.

#### E7. A test login from the command line

- On the `dev` stage only (a template condition on `Stage`), the app client also allows plain
  username-and-password login (`ALLOW_USER_PASSWORD_AUTH`). New script
  `scripts/aws-dev-token.sh <email>` asks for the password without echoing it and prints an access
  token, for `curl` checks. Trade-off: the dev pool accepts a weaker login method; see C27.
- **Tests:** none locally (it's Cognito configuration); deployment check D3.

#### E8. Template checks before deploying, and the walkthrough

- `scripts/check-template.sh` runs `sam validate --lint` on the template. It needs the SAM CLI,
  which you install once (walkthrough). Not part of `cargo test`: it needs a separate tool.
- New `infra/README.md`: the first-deployment walkthrough (account safety, tools, credentials,
  build both Lambdas, check, deploy, write the page config, create a user, the checks below,
  costs, tearing down).
- Correct [backend/README.md](../../backend/README.md)'s out-of-date "What's not built yet" list
  (`GET /export` and downloading Cognito's keys are built) and the template's header comment.

#### E9. A switch to log S3 notifications, for capturing one real sample (C24, C33)

**Status:** built and tested on this machine 2026-10-01 (commits `4e6d29e`, `f34a7dd`); not yet
run on AWS. Chosen by you on 2026-10-01 over logging every notification: the log stays off except
while a sample is being captured. All suites pass: 339 Rust. `aws_settings.rs` and
`s3_trigger.rs` stay at 100% line coverage; `scripts/check-template.sh` passes; both Lambdas build
for ARM. The capture step is "Capturing a real S3 notification for the tests" in
[infra/OPERATING.md](../../infra/OPERATING.md) (until 2026-10-02, step 9 of infra/README.md).

Differences from the design below:
- **The log-read-process step is a library function**, `s3_trigger::handle_raw_s3_event`, taking
  the logging setting and a function to write each line; the binary passes `println!`. Tests then
  check that the switch on logs exactly one redacted line before processing, that off logs nothing,
  and that an unreadable notification is logged and then refused. This narrows C36 to the binary's
  three lines of wiring.
- **The template quotes `"off"` and `"on"`.** Unquoted, some YAML readers take them as booleans,
  and the Lambda would receive `false` or `true` and refuse to start. A template test checks the
  quotes stay.

**Why:** C24 replaces the tests' sample S3 notification with a real one from the first deployment.
The processing Lambda doesn't record what it receives, so there is nothing to copy one from.

**Design:**

- **A template parameter, `LogS3Events`**, allowed values `off` and `on`, default `off`. It sets
  `TIMELINE_LOG_S3_EVENTS` on the processing function only. The API function never logs requests,
  so no login token can reach the logs through this.
- **Read once at start-up into a type, not a bare string** (new `EventLogging { Off, On }` in
  [aws_settings.rs](../../backend/timeline-api/src/aws_settings.rs)): `on` and `off` exactly; a
  missing variable is `Off`, so local runs and tests need nothing; anything else stops start-up
  with a message naming the value, rather than quietly logging or not.
- **Log what AWS actually sent, not a re-encoded copy.** The `aws_lambda_events` type for an S3
  notification drops any field it doesn't declare, so logging after reading into it would lose
  exactly the differences a real sample is meant to show. Instead, the processing binary receives
  the notification as plain JSON (`serde_json::Value`), logs it if the switch is on, then reads it
  into the `S3Event` type for `handle_s3_event`. A notification that can't be read as an `S3Event`
  is an error naming what failed, so Lambda retries it and the log shows why (CLAUDE.md's
  exception rule 3).
- **The uploader's IP address is removed before logging.** New function
  `redact_s3_event(value) -> Value` in [s3_trigger.rs](../../backend/timeline-api/src/s3_trigger.rs)
  replaces each record's `requestParameters.sourceIPAddress` with `"REDACTED"` and changes nothing
  else. Everything else in the notification (bucket name, which includes your account number; the
  object key, which includes the user's Cognito ID; AWS's request IDs) stays in the log: the log is
  in your own account, and those values are cleaned before anything is committed (below).
- **One log line per notification:** `s3 event (sourceIPAddress removed): <JSON>`, so `sam logs
  --filter "s3 event"` finds it.
- `handle_s3_event`'s signature doesn't change, so the committed tests in
  `tests/s3_trigger.rs` are untouched.

**Capturing (a new step in [infra/README.md](../../infra/README.md)):** redeploy with
`LogS3Events=on`, upload one small export through the page, copy the line from `sam logs`, then
redeploy with `LogS3Events=off`. Then, with me: replace the account number, bucket name, user ID,
upload ID, principal IDs and request IDs with obvious placeholders; replace
`backend/timeline-api/tests/fixtures/aws-samples/example-s3-event.json` with the result and record
where it came from in that folder's README; run the tests. You review the cleaned file before it's
committed.

**Tests:**
- `redact_s3_event`: the IP address is replaced in every record; the result equals the input with
  only that field changed; a notification without `requestParameters`, or with no records, comes
  back unchanged, not as an error.
- `EventLogging`: `on`, `off`, missing (`Off`), and refused values (`ON`, `yes`, empty), each refusal
  naming the value.
- The template: `LogS3Events` exists with default `off` and allowed values `off` and `on`, and only
  the processing function's settings refer to it (a text check, like the others).
- **Not tested locally:** the binary's own wiring (whether it logs when the switch is on), for the
  same reason `main.rs`'s isn't: it only runs inside Lambda. The capture step itself shows it works.

**Done means:** all suites pass; the changed modules stay at 100% line coverage;
`scripts/check-template.sh` passes; the capture step is in `infra/README.md`.

#### E10. The page's full address as the template's input, for pages behind a forwarding path (C39)

**Why:** on 2026-10-01 the first deployed login failed with Cognito's `redirect_mismatch`. The
user's browser runs on another machine and reaches the page through VS Code's port forwarding over
Tailscale, at `https://dev.tail13dce8.ts.net/proxy/8000/timeline.html`. The page sends Cognito its
own address without the query string
([cognito-login.js:27](../../frontend/infra/cognito-login.js#L27)):
`https://dev.tail13dce8.ts.net/proxy/8000/timeline.html`. The template, though, builds the only
allowed callback as `${FrontendOrigin}/timeline.html`, which cannot contain the `/proxy/8000`
part. Setting `FrontendOrigin` to `https://dev.tail13dce8.ts.net/proxy/8000` would fix the callback
but break CORS, because a browser's `Origin` header never has a path. E5 originally specified a
full-address `FrontendUrl`; the build replaced it with `FrontendOrigin` (see the deviation note
under V2e's status, "`FrontendOrigin` instead of `FrontendUrl`"), which is what this undoes.

**Design:**
- **One parameter, `FrontendUrl`**: the page's full address, default
  `http://localhost:8000/timeline.html`. It replaces `FrontendOrigin`. `AllowedPattern`
  `^https?://[^/]+(/[^?#]*)?/timeline\.html$` refuses at deploy time anything without a scheme, with
  a query string or fragment, or not ending in `/timeline.html`, with a `ConstraintDescription`
  saying so. Cognito still allows plain `http` only for `localhost`; it enforces that itself.
- **Cognito's callback and logout addresses** are `!Ref FrontendUrl`, exactly.
- **The CORS origin is derived from it inside the template**, so the user enters one value:
  `!Join ["", [!Select [0, !Split ["/", !Ref FrontendUrl]], "//", !Select [2, !Split ["/", !Ref FrontendUrl]]]]`.
  Splitting `https://dev.tail13dce8.ts.net/proxy/8000/timeline.html` on `/` gives `https:`, an
  empty piece, then `dev.tail13dce8.ts.net`, so the result is `https://dev.tail13dce8.ts.net`; for
  the default it is `http://localhost:8000`. Used for the API's `AllowOrigins` and the bucket's
  `AllowedOrigins`. A `Condition` can't hold a string, so the expression is written in both places;
  the template test below checks they are identical.
- **Everything else that names the old parameter**: [infra/README.md](../../infra/README.md)'s
  step 5 answer becomes "`FrontendUrl`: the address you open `timeline.html` at, without
  `?deploy=…`", with the Tailscale address as a second example; step 6 says to open the page at that
  same address; D2's `curl` uses that origin; step 9's redeploy command uses `FrontendUrl`.
  [scripts/write-deploy-config.sh](../../scripts/write-deploy-config.sh)'s closing message stops
  naming `localhost:8000` and says "open your page with `?deploy=<stage>`".
- **A "page address changed" note in the README** (kept, single allowed origin, chosen by the user
  2026-10-01 over allowing any origin): if the page's address changes (a different forwarded port,
  a renamed Tailscale machine, another device), Cognito shows `redirect_mismatch` and API calls
  fail in the browser with a CORS error that the page reports only as a failed request. Both come
  from `FrontendUrl`, so the fix is one redeploy:
  `sam deploy --parameter-overrides Stage=dev FrontendUrl=<new address> LogS3Events=off`.
- **Redeploying the existing stack:** the local `infra/samconfig.toml` (written by
  `sam deploy --guided`, untracked) names `FrontendOrigin`; CloudFormation refuses an unknown
  parameter. The README tells the user to rerun `sam deploy --guided` once, which rewrites it.
  Renaming the parameter changes no resource's name, so the stack is updated in place, not
  replaced; the change set preview should show `Modify` on `UserPoolClient`, `RawUploadsBucket`,
  `HttpApi` and nothing else, and I'll check that with the user before they confirm.

**Tests:**
- A new template test file, `backend/timeline-api/tests/template_frontend_url.rs`, text checks in
  the style of `template_event_logging.rs`: `FrontendUrl` exists with the default and pattern
  above; `FrontendOrigin` appears nowhere; `UserPoolClient`'s callback and logout are
  `!Ref FrontendUrl`; the API's and bucket's origins are the same derivation expression.
- `scripts/check-template.sh` passes.
- **Not testable locally:** that CloudFormation evaluates the derivation as described. Checked on
  AWS by deployment checks D2 (the `curl` with the Tailscale origin gets the header) and D3 (login
  returns to the page).

**Done means:** all suites pass; `scripts/check-template.sh` passes; the README and script are
updated; after the user redeploys, the login returns to the page (D3).

#### What only the first deployment can check

Each of these is run by hand after `sam deploy`, and the results are recorded in an analysis
document in `docs/analysis/`.

| # | Check | How |
|---|---|---|
| D1 | AWS accepts the template, including the bucket-trigger permissions (E2) | `sam deploy` succeeds |
| D2 | The browser's `OPTIONS` check is answered without a login, from the allowed origin only (C26) | `curl -X OPTIONS` with and without the right `Origin`; then the page |
| D3 | A real Cognito login works: the hosted page, the code exchange, and the command-line script | page login; `aws-dev-token.sh` then `curl` |
| D4 | The API refuses no token, a bad token, and a token from another pool | `curl` |
| D5 | Uploading the ~60 MB synthetic export: processed within the memory and time set in E2; then a full detection pass, each page answered within API Gateway's 30-second limit | page upload with detection on; `sam logs` shows peak memory and duration for each function |
| D6 | The flag-handle secret reaches the API (C19) | a flag save from the page succeeds |
| D7 | The real tables' keys match the tests' assumption (C16) | the page's whole flow, and the DynamoDB console |
| D9 | Start-up time, including downloading Cognito's keys (C23); passes if each function's `Init Duration` is under 1 s (limit set by the user, 2026-10-02) | `sam logs`: the `Init Duration` line |

D8 was removed on 2026-10-01; it is now C24's follow-up. The other numbers are kept so earlier
references stay valid.

**Where things stand (hand-off, 2026-10-02).** Facts a new session needs that aren't elsewhere in
this plan:
- **Deployed:** the `dev` stack, last deployed 2026-10-02 16:47 UTC, includes the error handling
  from [the upload-processing plan](2026-10-02-upload-processing-failures.md) (its phase A:
  attempts counted and shown on the page, `RecordFailedUploadFunction` marking an upload failed
  after the last retry). It does **not** include the later `FailProcessing` test setting (built,
  committed, never deployed). Its address setting is `FrontendUrl =
  https://dev.tail13dce8.ts.net/proxy/8000/timeline.html` (E10), kept in `infra/samconfig.toml`,
  which is local and not committed.
- **Deployment checks so far:** written up in
  [docs/analysis/2026-10-02-deployment-checks-status.md](../analysis/2026-10-02-deployment-checks-status.md).
- **The intermittent upload failure** ("item not found" while saving reviews) is diagnosed and
  measured: [read-after-write analysis](../analysis/2026-10-02-read-after-write-experiment.md).
  It happens only when review rows are new, so re-uploading the same file can't show it; review
  rows were deleted by hand on 2026-10-02 to reproduce it. The error handling was tested live the
  same day: retries shown with the failing row, then the upload marked failed after 3 attempts.
  **Next in that plan:** its §1 fix (approved; the user wanted the error handling tested first,
  which is now done), then its §3b failure-message change and §4 survey.
- **C38 is answered:** the Rust AWS SDK uses `aws login` credentials once `aws-config`'s
  `credentials-login` feature is on (found 2026-10-02; `timeline-storage`'s dev-dependency has
  it).
- **Working environment:** the user's browser is on another machine, reaching pages through VS
  Code's forwarding over Tailscale, never `localhost`. `aws login --profile timeline --remote`
  lasts under a day; Cognito's sign-in lasts an hour, so sign in afresh before a test (see the
  stale sign-in item below). The browser tests need Node 20 (`~/.nvm/versions/node/v20.20.2/bin`).
  The Rust build folder grows to about 30 GB and filled the disk on 2026-10-02; `cargo clean`
  fixes it.

**After the deployment checks (added at the user's request, 2026-10-02):**
- **Upload processing: the rest of its plan.** [2026-10-02-upload-processing-failures.md](2026-10-02-upload-processing-failures.md):
  phase A done and deployed; §1 fix next (see above); §3b, §4, and the optional §6 after.
- **A repeat-deployment guide and two helper scripts.**
  [2026-10-02-deployment-operating-guide.md](2026-10-02-deployment-operating-guide.md): outline
  only, holds the 2026-10-02 deployment lessons not written anywhere else; to be rewritten into a
  full plan.
- **A development-only "delete my earlier data first" checkbox.**
  [2026-10-02-dev-delete-before-load.md](2026-10-02-dev-delete-before-load.md): proposed, not
  approved.
- **Rewrite the load screen.** **Replaced** (2026-10-05, the user's decision) by [2026-10-05-screen-flow.md](2026-10-05-screen-flow.md): its Sign-in and Upload pages. The Load button sits far from the file chooser it depends on,
  separated by the sign-in area and a long paragraph about scanning; the user found the page
  confusing. Needs its own plan before any change.
- **A restore screen with a Stop button** (requested 2026-10-02). **Replaced** (2026-10-05, the user's decision) by [2026-10-05-screen-flow.md](2026-10-05-screen-flow.md): its loading modal, which blocks the page during a restore and has no Stop button. Today the page restores your last
  session quietly behind the load screen, which stays usable, so on 2026-10-02 a new load was
  started during a restore and the restored old data briefly appeared as if it were the new file.
  Instead, while a restore runs, show a restore screen (who is signed in, the download's
  progress) with a **Stop** button that cancels the restore and shows the load screen. Needs its
  own plan before any change.
- **A leftover "Picked up where you left off" notice** (found 2026-10-02). **Replaced** (2026-10-05, the user's decision) by [2026-10-05-screen-flow.md](2026-10-05-screen-flow.md), which removes the notice and "Load a different file". The notice is shown
  when the page opens and restores your last session
  ([load-flow.js:98](../../frontend/ui/load-flow.js#L98)), and only its own dismiss button hides it
  ([status-indicators.js:85-91](../../frontend/ui/widgets/status-indicators.js#L85-L91)); after
  "Load a different file" and a fresh load it still claims the old session was restored.
  **Proposed fix:** a `hideRestoredNotice()` next to `showRestoredNotice` in
  status-indicators.js, called by the "Load a different file" handler
  ([main.js](../../frontend/main.js)) and at the start of every load in `handleLoadClick`.
  *Reuse check:* no existing function hides the notice; the new one sits beside the one that
  shows it. *Test:* a browser test that restores a session (notice shown), clicks "Load a
  different file" (notice gone), loads a file (still gone).
- **Sign-in that goes stale** (found 2026-10-02; to be done after the deployment checks, at the
  user's request). "Signed in as …" is worked out once when the page loads
  ([login-panel.js:23-27](../../frontend/ui/login-panel.js#L23-L27)), while Cognito's access
  token lasts an hour (template `AccessTokenValidity: 1`). After that, every request fails with
  "sign in first" ([api-client.js:91](../../frontend/infra/api-client.js#L91)) under a "Signed
  in" label, and flag saves fail the same way. E5 said the page would send you through sign-in
  again; it doesn't. **Proposed fix:**
  1. *Renew quietly.* Cognito issues a refresh token with the authorization-code sign-in (believed
     to last 30 days by default; unverified). `oidc-client-ts`, already used
     ([cognito-login.js](../../frontend/infra/cognito-login.js)), can trade it for a new access
     token (`signinSilent`, or `automaticSilentRenew: true`, today `false`). `accessToken()` tries
     a renewal when the token has expired, before answering "none".
  2. *When renewal fails, say so.* The existing `refresh()` in login-panel.js is called whenever
     the load screen is shown (including "Load another file") and after any request is refused
     for an expired login, and shows "Your sign-in expired — sign in again" with the Sign in
     button, instead of a stale "Signed in".
  3. *Requests made after expiry* (upload, flag save, export) show that same message rather than
     "sign in first".
  4. *Tests:* a browser test with the existing stand-in Cognito server (`e2e/cognito-standin.js`)
     issuing a short-lived token and a refresh token: the label changes on expiry when renewal is
     refused; renewal keeps you signed in when it isn't; a flag save after expiry shows the expiry
     message. Unit tests for any new pure logic.
  - *Redundancy check:* renewal comes from the sign-in library already in use, and the status
    display reuses `refresh()`; nothing new duplicates existing code.
  - *To verify first:* that this Cognito client actually issues refresh tokens, and the
    library's renewal call against the stand-in server.

**Done means:** all suites pass (Rust, frontend unit, browser); the new modules are at 100% line
coverage; `scripts/check-template.sh` passes; `infra/README.md` exists. V2 itself is done only
after D1–D7 and D9 are run on a real deployment and recorded.

### V3 — Bedrock-based classification
**Reference implementation.** The browser-side "Classify with AI" code is deleted from the
working tree by [2026-09-30-split-timeline-script.md](completed/2026-09-30-split-timeline-script.md), because it
cannot work from a page served over HTTP. The links in this section are pinned to commit
`64996c5`, the last commit to change `timeline.html` before that deletion. They show the code
exactly as it last ran:
- model id and batch size: [timeline.html:1690-1691](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1690-L1691)
- prompt building and escaping: [timeline.html:1704-1740](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1704-L1740)
- the API call and its response checks: [timeline.html:1741-1797](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1741-L1797)
- retry once per batch: [timeline.html:1798-1808](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1798-L1808)
- the batch loop, with systemic-failure abort and checkpointing: [timeline.html:1809-1901](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1809-L1901)
- the `window.storage` checkpoint: [timeline.html:1580-1623](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1580-L1623)

**Adds**: server-side port of `classifyBatchWithAI`/`classifyBatchWithRetry`
([timeline.html:1741-1808](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1741-L1808)) calling `aws-sdk-bedrockruntime`'s `converse`
API instead of a client-side `fetch()` to `api.anthropic.com` — the direct fix for the CORS/no-API-
key dead end at [timeline-project-decisions.md:388-393](../../timeline-project-decisions.md#L388). Batch
orchestration via the SQS-checkpoint design (§1.1). **Prompt hardening is preserved verbatim**:
the XML `<message index="N">` tags and `escapeForPromptTags`
([timeline.html:1704-1740](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1704-L1740)) carry over unchanged, not redesigned.

**Files/modules**: `backend/timeline-core/src/classify.rs` (prompt building, pure/testable),
`backend/timeline-api/src/bedrock.rs` (SDK call + retry/checkpoint), new SQS queue, new
`ClassificationRuns` DynamoDB table (mirrors the "save every 5 batches" pattern,
[timeline-project-decisions.md:357-363](../../timeline-project-decisions.md#L357)).

**AWS resources**: SQS queue, Bedrock model access enabled on the account, `ClassificationRuns`
table, IAM scoped to `bedrock:Converse` on the specific model ARN.

**Running the application locally with real Bedrock** (added 2026-10-01 at the user's request; see
C37). The local server (`cargo run -p timeline-api`, the V2a harness) must be able to classify
with real Bedrock, so the user can try classification from `timeline.html` on their own machine.
Everything except the model call stays local: in-memory storage, `_dev/login`, the
`_dev/local-storage` upload path.
- **A switch picks the classifier**: an environment variable `TIMELINE_CLASSIFIER` with two
  values, `heuristic` (default, no AWS, no cost) and `bedrock`. Parsed once at startup into an
  enum; any other value stops startup with an error naming the bad value.
- **Credentials**: with `bedrock`, the server uses the AWS SDK's standard credential lookup, which
  picks up what `aws login` saved. No access keys. If none are found, startup fails with that
  message rather than failing on the first classification.
- **No SQS locally**: like `process_upload` in V2a, the local route calls the batch loop directly
  in the background instead of going through the queue. The checkpoint is written to the in-memory
  store, so the "save every 5 batches" logic still runs.
- **Cost guard**: the startup log line states that Bedrock is on and every call costs money.

**Tests** (per §2.1, no LocalStack Bedrock emulation available):
- `wiremock` unit tests: prompt-building snapshot tests (`insta`), and response-parsing tests
  covering every documented failure mode at
  [timeline.html:1754-1795](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1754-L1795) (network error, non-JSON response, API-level
  error, array-length mismatch, non-array response) — these were real bugs once
  ([timeline-project-decisions.md:369-382](../../timeline-project-decisions.md#L369)) and must not
  regress.
- One verified real communication sample: a captured `Converse` request/response pair from a real
  dev-account Bedrock call, personal content replaced with synthetic-but-structurally-identical
  text, checked in at `backend/tests/fixtures/bedrock_converse_sample.json`.
- A small number of real-Bedrock integration tests. They are checked-in test code, but marked
  `#[ignore]` so the ordinary `cargo test --workspace` skips them (each call costs money). They run
  only on demand, with `cargo test --workspace -- --ignored`, after `aws login`. Claude runs them
  only when the user asks for that in the same conversation. One of them drives the local server
  with `TIMELINE_CLASSIFIER=bedrock` through its HTTP routes (upload, start classification, read
  flags), so Bedrock is tested as part of the application, not only as an isolated call.
- **Manual check**: with `TIMELINE_CLASSIFIER=bedrock`, the user uploads a small export in
  `timeline.html` against the local server and classifies it.
- Regression tests: retry-once-per-batch, abort-after-first-systemic-failure, positional-partial-
  application ([timeline-project-decisions.md:307-310](../../timeline-project-decisions.md#L307)) ported
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
([timeline-project-decisions.md:400-402](../../timeline-project-decisions.md#L400)).

**AWS resources**: WAF WebACL, CloudWatch alarms + budget/cost-anomaly detection, API Gateway
usage plans/API keys per entitlement tier.

**Tests**:
- Load tests simulating concurrent uploads/classification runs (tool choice — e.g. `k6` or
  `oha` — TBD at implementation time; license-check before adopting, per project convention).
- Run this repo's `/security-review` skill against the full backend diff; specifically an
  adversarial test crafting a message that looks like a `</message>` closing tag, targeting the
  exact prompt-injection bug class already found once
  ([timeline-project-decisions.md:378](../../timeline-project-decisions.md#L378)).
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
user" from a convention (as it is today, [frontend/core/export-format.js:56-66](../../frontend/core/export-format.js#L56-L66)) into
something a unit test can assert structurally. `effectiveFlag()`'s four-state matrix
([timeline-project-decisions.md:264-276](../../timeline-project-decisions.md#L264)) stays a
**client-side** pure function computed from the two already-fetched fields — the `SHOW_AUTO`/
`SHOW_USER` toggles need to feel instantaneous, and this is genuinely presentation logic (deciding
what to *display*), not new computation on raw data. Server-side Analytics aggregates (friction
ranking, flag-rate trend) use the Rust port of the same logic since those already require a round
trip.

### 4.2 Dedup semantics
Runs exactly once, server-side, at upload time, mirroring the existing format-v2 rule (bare array
= unprocessed = dedup runs; wrapped-with-version-marker = already deduped,
[timeline-project-decisions.md:81-87](../../timeline-project-decisions.md#L81)). The client never
re-implements this.

### 4.3 Local-timezone session bucketing
`buildBlocks()` ([frontend/core/blocks.js:38-67](../../frontend/core/blocks.js#L38-L67)) fuses two things with
different timezone sensitivity — split them:
1. **Gap-based session splitting** (≥15 min since previous message) is timezone-agnostic — a delta
   between two instants. **Moves to the Rust backend** as `timeline_core::build_blocks`, UTC in,
   UTC session-boundary timestamps out.
2. **Which calendar day a session renders under** (`localDateKey()`,
   [frontend/core/blocks.js:11-16](../../frontend/core/blocks.js#L11-L16)) is timezone-sensitive, and is exactly the
   logic whose earlier server-side-in-UTC implementation caused a real bug ("Calendar bars ran
   past the edge of their day,"
   [timeline-project-decisions.md:371](../../timeline-project-decisions.md#L371)). **Stays client-side**:
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
([timeline.html:1704-1740](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1704-L1740),
[timeline-project-decisions.md §5.4/§10](../../timeline-project-decisions.md#L278)) is preserved
verbatim in the Rust port, plus the V5 adversarial test named above.

---

## 5. License Notes (data assets, not just crates)

- **The ~64,000-word English dictionary** ([timeline.html:930-64830 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L930-L64830), sourced
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
  [CLAUDE.md](../../CLAUDE.md) already uses for
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
  [timeline.html:1-849](../../timeline.html#L1-L849) and
  [frontend/ui/](../../frontend/ui/) (charts, markdown-lite renderer, tab
  switching, cross-navigation).
- `infra/` — `template.yaml` (AWS SAM).
- `timeline-project-decisions.md` stays at the repo root as the canonical constraint log; this
  plan doesn't move it.

---

## 7. Rough Cost Shape (order of magnitude, not a bill)

Using the sample dataset (~2,200 messages after dedup / 4,482 raw / 60MB,
[timeline-project-decisions.md:66-70](../../timeline-project-decisions.md#L66)):

| Component | One full session (upload + review + one classify pass + export) |
|---|---|
| S3 storage (~120MB) | ~$0.003/month — negligible |
| DynamoDB (~2,200 items) | ~$0.01 — negligible |
| Lambda (parsing/dedup) | fractions of a cent |
| API Gateway | fractions of a cent |
| **Bedrock classification (dominant cost)** | **~$0.70–$1 at Claude Haiku pricing** (consistent with the original design-time estimate, [timeline-project-decisions.md:295-297](../../timeline-project-decisions.md#L295)); meaningfully more at Sonnet-class models — re-check the exact per-token cost of whichever model ID is chosen at implementation time, pricing moves. |
| Stripe fee on $5 | ~$0.445 (2.9% + $0.30) |

**Net**: roughly $1–2 total cost against a $5 charge for one pass on this sample size — real
margin, but it narrows for larger files and is why the gating model caps each $5 to exactly one
pass rather than unlimited reruns (the existing "refresh detection" feature,
[timeline-project-decisions.md:97-101](../../timeline-project-decisions.md#L97), is a real, already-
designed way a user could otherwise re-trigger the expensive part for free).

**Uncertainty flags**: token-count assumptions are estimated from the existing batch design (30
messages/call, ~300-token prior-reply context, [timeline.html:65435 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65435)), not
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
[backend/timeline-core/tests/fixtures/sample_conversations.json](../../backend/timeline-core/tests/fixtures/sample_conversations.json)
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
([this plan §3 (line 137)](2026-09-09-rust-aws-backend-migration.md#L137), V1 "Anger detection" bullet) — includes a
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
([timeline.html:64906-64940 in the first commit, since deleted](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L64906-L64940)), ported faithfully to Python and verified
against its own logic, against the full uploaded `conversations.json` gave **4,457 raw messages,
4,451 after dedup, 6 dropped across 3 of 117 conversations** — not the **31 of 4,482** stated at
[timeline-project-decisions.md:66-70](../../timeline-project-decisions.md#L66). Real, measured mismatch,
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
correct the statistic in [timeline-project-decisions.md:66-70](../../timeline-project-decisions.md#L66)
is your call, not mine. Trigger: your decision on whether to update that line — nothing else
depends on resolving this further.

### C9 [RESOLVED]: Real personal conversation content was sitting uncommitted in the repo
Original concern: I flagged that your uploaded `conversations.json` (real personal Claude
conversation content, 64.7MB) was sitting untracked at the repo root, and that it shouldn't be
committed to git history by accident. **Resolution**: you removed the file yourself after I
raised it. The only conversation-derived content still in the repo is the trimmed, already-scoped
fixture at
[backend/timeline-core/tests/fixtures/sample_conversations.json](../../backend/timeline-core/tests/fixtures/sample_conversations.json)
— if different or fresher example data is ever needed (e.g. to investigate C8 further), you'll
supply it again rather than me generating or requesting it independently.

### C10 [RESOLVED]: Testing the real S3/DynamoDB adapters without a container
V2a's in-memory adapters intentionally avoid AWS/containers for the routine upload-view-flag loop
(see V2a above), but `timeline-storage/src/s3.rs` and `timeline-storage/src/dynamo/*.rs` have never
run against anything real — confirmed directly, not from memory, before writing this:
- **DynamoDB**: genuinely Docker-free. AWS publishes "DynamoDB Local" as a downloadable JAR
  (needs JRE 17+, `openjdk-17-jre-headless` available via `apt` but not yet installed) — no Docker
  required. `java -jar DynamoDBLocal.jar -sharedDb` on `localhost:8000`; the real
  `aws-sdk-dynamodb` client just needs its endpoint pointed there with fake credentials. This would
  exercise `DynamoConversationsTable`/`DynamoMessageFlagsStore`'s actual code, not a stand-in.
- **S3**: no equally clean answer yet. MinIO was the obvious candidate, but its licensing has
  visibly changed since — its docs now point to a commercial "AIStor" product under a proprietary
  license, not the free server I'd have recalled from memory. Confirmed this by checking MinIO's
  own current docs rather than asserting outdated information. No AWS-official S3-local equivalent
  to DynamoDB Local exists. Other options (e.g. `s3rver`, Node-based, MIT-licensed) are unverified
  candidates, not yet checked for current maintenance status or fidelity.

**Mitigation in plan:** none yet — this is real, unstarted work.
**Open:** research and, if viable, wire up DynamoDB Local (straightforward) and a real Docker-free
S3-compatible option (needs more investigation) as a dedicated increment after V2a's read+write
flow is committed. Explicitly **not part of V2a** — V2a stays in-memory-only, deliberately, per
the container-free/AWS-free design already agreed for routine testing.
**Update 2026-09-30:** the increment is now designed in §V2b. The S3 research is done. MinIO is
also archived, and `s3rver` is archived too. §V2b compares four candidates and recommends `s3s` +
`s3s-fs`, which you chose on 2026-09-30. The trigger to mark this resolved: §V2b's "done means"
list is met.
**Status 2026-10-01:** §V2b is implemented (commits `4d986c1`, `64e79da`, `cee5dae`, `8abceaf`,
`d96a09b`). All suites pass. `s3.rs` has 100% line coverage. The two DynamoDB files have three
lines between them that no test reaches, because I don't think any row DynamoDB can return
reaches them. Still `[OPEN]` until you decide on those lines and on the findings listed in the
implementation report: a flags-adapter difference between the fake and DynamoDB, malformed upload
ids being silently dropped, and the readiness check differing from this plan.

**Resolution (2026-10-01):** the findings listed above were handled in §V2c, and the readiness
check now waits for a real `ListTables` request to succeed instead of an open port, as §V2b
specified (commit `efa9a8b`, in `timeline-storage/tests/support/dynamodb_local.rs`). A
throwaway check confirmed a plain web server on the port is refused with a message naming the
address.
### C15 [RESOLVED]: DynamoDB Local is not under a permissive open-source license
AWS provides it free, but under its own license. It isn't one of the MIT/BSD/Apache-2.0/ISC
licenses that the reuse rule in [CLAUDE.md](../../CLAUDE.md) lists.

**What the license says** (the "Amazon DynamoDB Local License Agreement", `LICENSE.txt` in the
version 3.3.1 download; I read it on 2026-09-30 and am not a lawyer):
- **You need an AWS account in good standing** to use it at all. The grant is personal and can't
  be transferred, so every person or machine running the tests is its own licensee.
- **Allowed use:** installing it on computers you own or control, only "for your internal
  business purposes" and "in connection with the Services", meaning AWS. Testing code that will
  run against real DynamoDB fits both.
- **Forbidden:** building it into, or compiling it with, our own programs; redistributing it
  (so no checking it into git, and no shipping it inside an image or installer); modifying it;
  reverse engineering it.
- **AWS can change or end it at any time**, including making the software stop working, and can
  change the terms by posting new ones. Continuing to use it counts as accepting them.
- You indemnify AWS (you cover its costs if your use leads to a claim). AWS's liability is capped
  at $50.
- **Telemetry:** the license doesn't mention it, but the release notes say version 2.1.0 added
  telemetry, and the program's help text offers `-disableTelemetry`. The download bundles AWS's
  Pinpoint client library, which suggests the data goes to AWS Pinpoint. That is inferred from the
  file name; I have not traced it.
- The bundled third-party libraries list Apache-2.0, MIT and EPL licenses. I found no GPL text.
  That wouldn't matter anyway, since nothing is shipped.

**Mitigation in plan:** §V2b runs it only as a separate program, fetched per machine into a
folder git ignores, never linked or shipped, with `-disableTelemetry` on every launch, and with
the version pinned by checksum. **Open:** the AWS-account requirement. You'll need an account for
§V2's deploy anyway, but every machine that runs the tests, including any future CI (automated
test) server, needs one too. Trigger: your approval of §V2b. If that's unacceptable, the
alternative is Moto server (Apache-2.0), which also fakes DynamoDB.
**Resolution:** on 2026-10-01 you accepted the AWS-account requirement. The rules that follow from
the license are listed in [§V2b's license bullet (line 581)](2026-09-09-rust-aws-backend-migration.md#L581).

### C16 [OPEN]: The test tables' key layout is copied from the SAM template, not read from it
§V2b's test helper creates tables with `pk`/`sk` string keys to match
[infra/template.yaml](../../infra/template.yaml). If the template changes, the tests won't notice.
Reading the template directly is harder than it looks: it uses CloudFormation tags such as
`!Sub`, which ordinary YAML readers reject. **Mitigation in plan:** the layout lives in one helper
with a comment pointing at the template lines. **Open:** trigger is the first change to a table's
key layout in the template, or the real-AWS run in §V2, which would catch a mismatch for real.

### C17 [RESOLVED]: Requiring Java for `cargo test --workspace`
§V2b makes the DynamoDB tests fail, not skip, when Java or the DynamoDB Local JAR is missing. That
means a fresh machine can't run the full test suite until the one-time setup is done. The
alternative is a Cargo feature, off by default, that turns these tests on. That would keep plain
`cargo test` working with no setup, but it risks the tests silently not running, which is how the
adapters went untested in the first place. **Mitigation in plan:** the failure message names the
exact setup command. **Open:** your call. Trigger: your review of §V2b, or the first time the setup
requirement gets in the way (for example, a CI machine without Java).
**Resolution:** on 2026-10-01 you chose to have the tests fail. That is what
[§V2b (line 575)](2026-09-09-rust-aws-backend-migration.md#L575) already says. The Cargo-feature
alternative is not adopted.

### C18 [RESOLVED]: DynamoDB adapters fill in defaults for missing or malformed values
Found while implementing §V2b: six places in `conversations_table.rs` and the flag-reading code in
`message_flags_table.rs` replace a missing or wrong-typed stored value with a default, or drop it.
Separately, three sort-key checks can't be reached by any test; on 2026-10-01 you decided they stay,
commented as currently unreachable backstops (§V2c). **Mitigation in plan:** §V2c lists every
default-filling case and designs the fix. **Open:** trigger is your approval of §V2c.
**Resolution:** implemented in §V2c on 2026-10-01; see its status note ([line 696](2026-09-09-rust-aws-backend-migration.md#L696)).

### C19 [OPEN]: The flag-handle key on AWS is designed but can't be checked until deployment
§V2c's flag-handle key comes from AWS Secrets Manager through the SAM template, which has never been
deployed. **Mitigation in plan:** the Lambda refuses to start without the key, so a broken setup
fails loudly instead of silently. A test covers that. **Open:** trigger is the first real `sam
deploy` in §V2. Changing the key later invalidates every handle already issued until the page
reloads; a key-rotation procedure is worth planning at that point.

### C20 [RESOLVED]: The extra size of the `/export` reply is estimated, not measured
About 100 bytes per message (a 36-character ID plus a 43-character handle and JSON punctuation),
roughly 450 KB for an export the size of the user's. **Mitigation in plan:** none needed to build
it. **Open:** measure the reply size in the `/export` test using the checked-in fixture, and report
the per-message figure. Trigger: if a real export's reply passes 5 MB, consider sending handles per
conversation on demand instead.
**Resolution:** measured on 2026-10-01 in `timeline-api/tests/flag_saves.rs`: 85 bytes per user
message (14 handles made a 1,302-byte reply). The 5 MB trigger would take about 61,000 user
messages; the reply carries handles for user messages only.

### C21 [RESOLVED]: The Lambda build still uses the in-memory stores and the dev login keys
Found while wiring the flag-handle key into
[main.rs](../../backend/timeline-api/src/main.rs): the Lambda branch calls the same
`build_local_state` as local dev, so it uses the in-memory stores, not the S3 and DynamoDB
adapters, and verifies logins against the throwaway dev keypair, not Cognito. Only the flag-handle
key differs (read from the environment since §V2c). A deployed Lambda would therefore lose all data
between instances and accept tokens signed by the dev keypair. **Mitigation in plan:** §V2d
designs the fix; nothing is deployed.
**Resolution:** implemented in §V2d on 2026-10-01 ([line 910](2026-09-09-rust-aws-backend-migration.md#L910)). **Open:** wire the real adapters and Cognito verification into the Lambda
branch before the first `sam deploy` in §V2. Trigger: the start of V2's deployment work.

### C22 [OPEN]: Cognito's keys are fetched once per Lambda instance
§V2d downloads the user pool's public keys at startup. When Cognito adds or rotates a signing key,
an instance started before the change would refuse tokens signed with the new key until it
restarts. **Mitigation in plan:** Lambda instances are short-lived, typically minutes to hours.
**Open:** re-download the keys when a token names a key ID the instance doesn't know, rate-limited
so a stream of bad tokens can't trigger a download per request. Trigger: the first deployment, or
any report of valid logins being refused.

### C23 [OPEN]: Downloading the keys adds to Lambda start-up time
One HTTPS request to Cognito per new instance. **Mitigation in plan:** none needed to build it.
**Open:** measure start-up time on the first deployment; if the download is a noticeable share,
bundle the keys into the deployment instead. Trigger: the first `sam deploy`.

### C24 [RESOLVED for the S3 notification; OPEN for the API request]: The sample AWS events come from a library, not from our deployment
E1 and E2 test with the sample events shipped in `aws_lambda_events` 0.16.1's `src/fixtures/`. They
match AWS's published formats as far as I've read them, but they weren't captured from this
project's API or bucket, so they aren't the verified samples CLAUDE.md asks for. **Mitigation in
plan:** the tests only rely on the fields the code reads (path, stage, authorizer claims; bucket and
key). **Open:** trigger is deployment check D8, which captures real events and replaces the samples.
**Update 2026-10-01:** D8 is no longer a deployment check; this critique holds it. What a real sample
adds that D3–D7 don't: the automated tests keep using what AWS really sends after future changes
(a library upgrade could change request handling again, as the stage name did), and the real
values of fields the samples get wrong for our setup (the request sample's route key is
`$default`; ours will be per-method routes). **Chosen:** capture one real S3 notification during
the first deployment, clean it of the uploader's IP address and account details, and replace
`example-s3-event.json` with it. The API request stays the library's sample, because a real one
carries a login token. **Still open:** the request sample, until a library upgrade or a bug traced
to an event's format; and how the S3 notification is captured (C33).
**Resolution 2026-10-02 (S3 notification):** captured from the dev stack by README step 9, cleaned
of account number, bucket name, user and upload IDs, role ID, AWS request IDs and the file's eTag,
and reviewed and approved by the user before it replaced
`backend/timeline-api/tests/fixtures/aws-samples/example-s3-event.json`. The address the logger had
already removed is set to `127.0.0.1`, the library sample's address, so the redaction test has one
to remove and its check for that address stays meaningful; it is the only value AWS did not send. AWS sent `eventVersion` 2.6 and an `awsGeneratedTags`
block, which the library sample lacked; the upload processed to completion on the page, and the
backend tests pass with the new sample. `example-destination-failure.json`'s `requestPayload` now
carries the same sample (that file's other fields are still from AWS's documentation; see C5 of
`2026-10-02-upload-processing-failures.md`).

### C25 [RESOLVED]: Adding a field to `AppState` edits committed tests
E3 adds `upload_outcome_store` to `AppState`. Every test that builds an `AppState` by hand (at
least `tests/lambda_router.rs` and `tests/aws_state.rs`; I haven't listed all of them) needs one
more line to compile. No assertion changes. CLAUDE.md requires your approval to modify committed
tests. **Mitigation in plan:** the edits add the field and nothing else. **Open:** trigger is your
approval of this plan; approving it is taken as approving those one-line edits, unless you say
otherwise.
**Resolution:** approved with the plan on 2026-10-01; commit `0e0e49b` adds the field (and its
import, where missing) to the six files that build `AppState` by hand, nothing else.

### C26 [OPEN]: Whether API Gateway answers the browser's `OPTIONS` check is read from documentation
E6 relies on my reading of AWS's documentation that, with CORS configured and no route matching
`OPTIONS`, API Gateway answers it without running the login check. Not verified. **Mitigation in
plan:** the template uses explicit `GET`/`POST`/`PATCH` routes so no route matches `OPTIONS`.
**Open:** trigger is deployment check D2. If it's refused, the fallback is an explicit `OPTIONS`
route with no login requirement, answered by the Lambda.

### C27 [OPEN]: The dev stage allows plain password login through the API
E7 turns on `ALLOW_USER_PASSWORD_AUTH` on the `dev` stage so a token can be fetched from the command
line. That method sends the password to Cognito directly (over HTTPS) instead of proving it by
challenge and response. **Mitigation in plan:** `dev` only, by a template condition. **Open:**
trigger is putting anyone else's data in the `dev` stage, or creating any other stage.

### C28 [OPEN]: The flag-handle secret is visible in the API Lambda's configuration
The template passes the secret as an environment variable through a `{{resolve:secretsmanager:...}}`
reference, so anyone allowed to read the function's configuration can read the secret. **Mitigation
in plan:** the account currently has one person. **Open:** have the Lambda read the secret from
Secrets Manager at start-up instead (`aws-sdk-secretsmanager`, Apache-2.0). Trigger: anyone else
getting access to the AWS account, or the start of V4.

### C29 [RESOLVED]: A retried S3 event could store an upload twice
Original concern: Lambda retries a failed S3 event, so `process_upload` can run more than once for
one upload. **Resolution:** every write it makes replaces rather than adds, and E2 adds a test that
processing an event twice leaves the same stored data as once ([§E2 handler, line 1074](2026-09-09-rust-aws-backend-migration.md#L1074)).

### C30 [RESOLVED, cross-plan]: The page is still served from this machine
Original concern: after V2e the API, storage and logins run on AWS, but `timeline.html` is served
locally and pointed at the API. Nobody else can use it. **Resolution:** the page is hosted on S3
behind CloudFront by [2026-10-02-page-hosting.md](2026-10-02-page-hosting.md), deployed on
2026-10-02 with its deployment checks passed except H5 and H7
([analysis](../analysis/2026-10-02-page-hosting-deployment.md)).

### C31 [RESOLVED]: The processing Lambda would need Cognito settings it never uses
Original concern: `AwsSettings` refuses to load without the Cognito pool and client, so the
processing Lambda would need them set for no reason. **Resolution:** E2 adds `StorageSettings` for
the three storage names, shared with `AwsSettings` ([§E2 settings, line 1069](2026-09-09-rust-aws-backend-migration.md#L1069)).

### C32 [RESOLVED]: `oidc-client-ts` may not load without a build step
E5 assumes the library's prebuilt browser file works when loaded directly by the page. Unverified.
**Mitigation in plan:** checked first, before any other E5 work. **Open:** if it doesn't load, the
fallback is writing the code exchange by hand with the browser's built-in cryptography (two web
requests and one hash), which I'd bring back to you before doing. Trigger: the start of E5.
**Resolution:** its `dist/browser/oidc-client-ts.min.js` is one self-contained script defining the
global `oidc`, with `jwt-decode` bundled and no imports. Kept in `vendor/oidc-client-ts/` with both
licenses and its checksum; the browser tests load it from a plain `<script>` tag (commit `70db163`;
see the status note at [line 999](2026-09-09-rust-aws-backend-migration.md#L999)).

### C33 [OPEN]: Nothing captures a real AWS event
**Update 2026-10-01:** only the S3 notification is to be captured now (C24). The processing Lambda
doesn't log the events it receives, so this still needs a small code change; the choice of how is
open. Trigger: before the first deployment.
**Update 2026-10-01 (later):** you chose a switch, off by default, over always logging; designed in
§E9 ([line 1254](2026-09-09-rust-aws-backend-migration.md#L1254)). Stays open until E9 is built and
a real notification is captured.

D8 is meant to replace the library's sample events (C24) with sanitized copies of real ones. No
code records an incoming event, and logging whole events would also log every request's login
token. **Mitigation in plan:** the tests rely only on the fields the code reads. **Open:** choose a
way to capture one HTTP API event and one S3 event (for example, a temporary setting that logs an
event with its `authorization` header removed). Trigger: the first deployment, before D8.

### C34 [RESOLVED]: Copies of the bucket name in the template could drift apart
Original concern: to avoid the circular dependency (E2), the uploads bucket's name is written out as
text in five places instead of referred to. If one copy changed and another didn't, `sam validate
--lint` wouldn't notice; the deployment would succeed and uploads would then fail with "access
denied". **Resolution:** `timeline-api/tests/template_bucket_name.rs` checks every copy matches the
bucket's own name, and fails when one is misspelled (checked by misspelling one); commit `8717329`.

### C35 [RESOLVED, by avoidance]: Switching `LogS3Events` might reset the other deployment settings
E9's capture step redeploys with `--parameter-overrides LogS3Events=on`. I believe that, given on
the command line, it replaces every parameter override saved in `samconfig.toml` rather than adding
to it, so `Stage` and `FrontendOrigin` would have to be repeated; I haven't checked SAM's
documentation or tried it. **Mitigation in plan:** the README step will give the full command,
`--parameter-overrides Stage=dev FrontendOrigin=http://localhost:8000 LogS3Events=on`, which is
correct either way. **Open:** confirm when the capture step is first run. Trigger: that step.
**Resolution 2026-10-02:** not tested; the user repeated every setting, as README steps 9 and 10
direct, and chose to keep using that method. SAM's behavior with a single setting remains unknown;
reopen only if a README step stops repeating every setting.

### C36 [RESOLVED]: The switch's wiring in the binary is untested locally
Whether the processing binary actually logs when `LogS3Events` is `on` is only shown by running it
inside Lambda, like `main.rs`'s wiring. **Mitigation in plan:** the binary is kept to a few lines;
the redaction and the setting are tested. **Update 2026-10-01:** narrowed by building the
log-read-process step as a tested library function (see §E9's status note,
[line 1254](2026-09-09-rust-aws-backend-migration.md#L1254)); untested now is only the binary reading
the setting and passing `println!`. **Open:** trigger is the capture step: no log line means
the wiring is wrong.
**Resolution 2026-10-02:** the capture step ran on the dev stack with `LogS3Events=on` and
`sam logs` returned the `s3 event (sourceIPAddress removed):` line, with the address replaced by
`REDACTED`. The binary reads the setting and logs in Lambda.

### C37 [RESOLVED]: V3 had no way to use Bedrock from the locally running application
Original concern (raised by the user, 2026-10-01): V3 only described isolated real-Bedrock tests,
"run manually/nightly", which was vague about who runs them and when, and left no way to try
Bedrock classification through the application running locally. The user wants Bedrock tested as
part of the application.
**Resolution:** V3 gains a local-run subsection (a `TIMELINE_CLASSIFIER` switch, `aws login`
credentials, no SQS locally) and its test list now says exactly when the paid tests run and adds
one that goes through the local server's HTTP routes; see
[V3 local run (line 1376)](2026-09-09-rust-aws-backend-migration.md#L1376).

### C38 [OPEN]: The Rust AWS SDK may not read `aws login`'s saved credentials
`aws login` (AWS CLI 2.37.8, confirmed present by reading its help text) saves temporary
credentials in its own cache. I have not checked whether the Rust SDK's standard credential lookup
reads that cache. **Mitigation in plan:** if it doesn't, `eval "$(aws configure export-credentials
--format env)"` puts the same temporary credentials into environment variables, which the SDK does
read; still no access keys. **Open:** trigger is the first local run with
`TIMELINE_CLASSIFIER=bedrock`.

### C39 [RESOLVED]: The template can't describe a page served under a path
Original concern (found by the user's first deployed login, 2026-10-01): `FrontendOrigin` plus a
fixed `/timeline.html` can't express `https://dev.tail13dce8.ts.net/proxy/8000/timeline.html`,
so Cognito refused the return address. The build had swapped E5's `FrontendUrl` for an origin
without asking, and the README assumed the browser runs on the same machine as the server.
**Resolution:** E10 restores a full-address parameter and derives the origin from it; see
[E10 (line 1324)](2026-09-09-rust-aws-backend-migration.md#L1324).

### C40 [OPEN]: Whether VS Code's forwarding passes Cognito's return through intact
Cognito returns to the page with `?code=…&state=…`. I haven't checked that VS Code's `/proxy/8000/`
forwarding over Tailscale keeps the query string and serves `timeline.html` for it. The page
already loaded with `?deploy=dev` through it, which suggests query strings pass, but that isn't
the same request. **Mitigation in plan:** none needed if it works; if not, the page shows no
login and the address bar shows what arrived. **Open:** trigger is D3 after the redeploy.

### C41 [OPEN]: Cognito's sign-up emails land in spam
Observed by the user on the first deployed sign-up (2026-10-02): the confirmation-code email went
to the spam folder. The template leaves Cognito on its default sender, which (from memory, not
checked) sends from the shared address `no-reply@verificationemail.com` and is limited to about
50 emails a day per account. Friction for every new user, and a hard cap. **Mitigation in plan:**
none yet; the user can check spam. **Open:** send through Amazon SES from an address on a domain
the user controls (e.g. `no-reply@telotrope.ai`): DNS records proving the domain (SPF, DKIM,
DMARC), a request to move SES out of its sandbox (reviewed by AWS by hand), and the user pool's
`EmailConfiguration` set to `DEVELOPER` with the SES identity. Needs its own plan section and the
user's DNS access. Trigger: before anyone other than the user signs up.

### C11 [RESOLVED]: `UploadStatus`'s `Pending`/`Processing` are persisted but never read
Confirmed by `grep`, not assumed: no route reads `UploadRecord.status`, and `process_upload`
didn't branch on it either. Written by `create_pending`/`mark_processing`, read by nothing.
**Resolution:** applied the [§V2a-revision](#v2a-revision-what-each-store-actually-persists-and-why--a-design-review-found-real-problems)
redesign to code — `UploadStore`/`UploadRecord`/`UploadStatus` are gone, replaced by
[`UploadOutcomeStore`](../../backend/timeline-core/src/ports/uploads.rs#L43)'s
`record_outcome`/`get_outcome`, which only ever holds a terminal `Ready`/`Failed` value. `POST
/uploads` ([routes/uploads.rs](../../backend/timeline-api/src/routes/uploads.rs)) never
constructs or calls it at all now.

### C12 [RESOLVED]: `raw_object_key` is stored despite being a pure function of `(user_id, upload_id)`
`create_upload` computed it as `format!("raw/{user_id}/{upload_id}.json")`, then wrote it to
`UploadStore` and read it back in `export.rs` instead of recomputing it. **Resolution:** the
format string now lives in exactly one place,
[`raw_object_key`](../../backend/timeline-core/src/ports/uploads.rs#L27), and every caller that
needs the key — [`routes::uploads::create_upload`](../../backend/timeline-api/src/routes/uploads.rs#L41),
[`routes::export::export`](../../backend/timeline-api/src/routes/export.rs#L64), and
`processing::process_upload` — calls it fresh rather than reading a stored copy back.

### C13 [RESOLVED]: Port names overclaim what they persist
`ConversationStore` stores `ConversationSummary` (name, count, a foreign key) — never a
conversation's actual messages, which are never persisted as a structured thing at all, only
reconstructed by re-parsing the raw upload blob on demand. `UploadStore` doesn't store an upload's
content either — that's in `ObjectStore`; it stored a status/outcome record pointing at where the
content is. **Resolution:** renamed to
[`ConversationSummaryStore`](../../backend/timeline-core/src/ports/conversations.rs#L45) and
[`UploadOutcomeStore`](../../backend/timeline-core/src/ports/uploads.rs#L43), alongside C11/C12's
structural simplification (not a rename alone — `UploadOutcomeStore`'s contract is genuinely
smaller). The concrete adapters were renamed to match (`InMemoryConversationSummaryStore`,
`InMemoryUploadOutcomeStore`) so an adapter's own name no longer implies a trait that doesn't
exist.

### C14 [RESOLVED]: The ports' own shape was only explicable via AWS-specific reasoning, not domain terms
Original concern: explaining why three separate storage ports exist, and what each one's
persistence duration is for, required reciting DynamoDB's 400KB item cap and Lambda's
statelessness between invocations — meaning `timeline-core`'s ports weren't actually hiding
infrastructure from the domain layer the way the crate's own stated principle requires, even
though they contain zero AWS SDK imports (the letter of that rule was satisfied; the spirit
wasn't). **Resolution:** [§V2a-revision above](#v2a-revision-what-each-store-actually-persists-and-why--a-design-review-found-real-problems)
identifies the one fact that actually justifies durable storage here (cross-invocation memory
isn't guaranteed, stated in infrastructure-neutral terms: "readable by a process other than the
one that wrote it, at an unpredictable later time") and states plainly, rather than papering over,
what this repo's port design still doesn't fully solve: the ports still encode a real
infrastructure *requirement* (that's unavoidable — the domain genuinely needs durable, indexed
storage), just no longer the AWS-Lambda-specific residue (an unused status lifecycle, a
recomputable value stored as if it weren't) that was riding along with it.

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

- [timeline.html in the first commit](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html) — porting source. Key ranges, with where each is now:
  [64906-64990](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L64906-L64990) (dedup + flag load; dedup since deleted, flag load now [frontend/core/export-format.js:26-94](../../frontend/core/export-format.js#L26-L94)), [65193-65296](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65193-L65296)
  (`effectiveFlag`/`buildBlocks`/`attachFlags`, now [frontend/core/flags.js:12-62](../../frontend/core/flags.js#L12-L62) and [frontend/core/blocks.js:11-67](../../frontend/core/blocks.js#L11-L67)), [65417-65531](https://github.com/Telotrope/conversation-timeline/blob/4f3269c/timeline.html#L65417-L65531)
  (`classifyBatchWithAI`/`classifyBatchWithRetry`, since deleted).
- [timeline-project-decisions.md](../../timeline-project-decisions.md) — full constraint set; §2.3
  (dedup), §3 (sessions), §5.3 (four-state matrix), §10 (bug root causes) are most load-bearing.
- [CLAUDE.md](../../CLAUDE.md) — testing rigor, exception-handling, trust-boundary sanitization, and
  license-discipline rules every version's design above was checked against.
- New: `backend/timeline-core/src/{dedup,sessions,flags/heuristic,classify}.rs`,
  `infra/template.yaml`.
