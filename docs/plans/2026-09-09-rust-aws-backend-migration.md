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
([timeline.html:64963](timeline.html#L64963)) already knows how to consume — flags embedded as
`_claude_timeline_auto`/`_claude_timeline_user`, exactly the fields it already reads. So the only
thing that changes is *how the raw text reaches `parseUploadedConversations`*, not what happens to
it afterward. `CONVERSATIONS`/`MESSAGES`/`HUMAN_MESSAGES`/`BLOCKS` and every rendering function
stay untouched.

**New `handleLoadClick()` flow** ([timeline.html:65047](timeline.html#L65047) onward):
1. Read the chosen file's raw bytes client-side (same as today — needed to `PUT` them).
2. If no auth token is cached yet, call `POST /_dev/login` with a display name typed into one new
   text input (`id="devLoginSub"`, placed next to the existing file picker at
   [timeline.html:747-748](timeline.html#L747-L748)) to get one. Dev-only, matching the rest of
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
([timeline.html:65086](timeline.html#L65086) and
[timeline.html:65102](timeline.html#L65102)) and one save call in `setRowOverrides()`
([timeline.html:65392](timeline.html#L65392)):
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
- `setRowOverrides(id, changedType, changedValue)` ([timeline.html:65382](timeline.html#L65382))
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
  ([timeline.html:65392](timeline.html#L65392)) is replaced by this, not left calling
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

### V3 — Bedrock-based classification
**Reference implementation.** The browser-side "Classify with AI" code is deleted from the
working tree by [2026-09-30-split-timeline-script.md](2026-09-30-split-timeline-script.md), because it
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
key dead end at [timeline-project-decisions.md:388-393](timeline-project-decisions.md#L388). Batch
orchestration via the SQS-checkpoint design (§1.1). **Prompt hardening is preserved verbatim**:
the XML `<message index="N">` tags and `escapeForPromptTags`
([timeline.html:1704-1740](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1704-L1740)) carry over unchanged, not redesigned.

**Files/modules**: `backend/timeline-core/src/classify.rs` (prompt building, pure/testable),
`backend/timeline-api/src/bedrock.rs` (SDK call + retry/checkpoint), new SQS queue, new
`ClassificationRuns` DynamoDB table (mirrors the "save every 5 batches" pattern,
[timeline-project-decisions.md:357-363](timeline-project-decisions.md#L357)).

**AWS resources**: SQS queue, Bedrock model access enabled on the account, `ClassificationRuns`
table, IAM scoped to `bedrock:Converse` on the specific model ARN.

**Tests** (per §2.1, no LocalStack Bedrock emulation available):
- `wiremock` unit tests: prompt-building snapshot tests (`insta`), and response-parsing tests
  covering every documented failure mode at
  [timeline.html:1754-1795](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1754-L1795) (network error, non-JSON response, API-level
  error, array-length mismatch, non-array response) — these were real bugs once
  ([timeline-project-decisions.md:369-382](timeline-project-decisions.md#L369)) and must not
  regress.
- One verified real communication sample: a captured `Converse` request/response pair from a real
  dev-account Bedrock call, personal content replaced with synthetic-but-structurally-identical
  text, checked in at `backend/tests/fixtures/bedrock_converse_sample.json`.
- A small number of real-Bedrock integration tests, run manually/nightly given per-call cost.
- Regression tests: retry-once-per-batch, abort-after-first-systemic-failure, positional-partial-
  application ([timeline-project-decisions.md:307-310](timeline-project-decisions.md#L307)) ported
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
([timeline.html:1704-1740](https://github.com/Telotrope/conversation-timeline/blob/64996c536e3c80f6de94bf96ef2941e2006c5264/timeline.html#L1704-L1740),
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

### C10 [OPEN]: Testing the real S3/DynamoDB adapters without a container
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
