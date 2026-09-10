# Backend workspace (V1 + V2)

Rust backend for the conversation-timeline tool, per
[docs/plans/2026-09-09-rust-aws-backend-migration.md](../docs/plans/2026-09-09-rust-aws-backend-migration.md).
Four crates:

- **`timeline-core`** (V1) — pure domain logic, no I/O: dedup, session
  splitting, flag heuristics, VADER sentiment, the export schema types, and
  the storage/auth "ports" (traits) the other crates implement.
- **`timeline-storage`** (V2) — adapters for those ports: real S3/DynamoDB
  clients, and in-memory fakes used for local dev and tests.
- **`timeline-auth`** (V2) — Cognito access-token verification.
- **`timeline-api`** (V2) — the axum app (5 routes so far), and the
  Lambda/local-dev entrypoint.

## Prerequisites

Rust toolchain (`rustc`/`cargo`, via [rustup](https://rustup.rs)) **and** a
C linker — `rustup` does not install one:

```
sudo apt install build-essential
```

## Building and running the real Lambda binary (`cargo-lambda`)

Optional — only needed to build/verify the actual Lambda artifact, not for
`cargo build`/`cargo test`/`cargo run` above. Install:

```
curl -fsSL https://cargo-lambda.info/install.sh | sh
```

`cargo-lambda` cross-compiles with [Zig](https://ziglang.org) as its linker
— **not Docker**. If `zig` isn't already on `PATH`, download a prebuilt
release for your platform from ziglang.org/download and add it to `PATH`;
there's no official system package on most distros. Then:

```
# Cross-compile the real Lambda artifact (ARM64, matching template.yaml):
cargo lambda build --release --arm64 -p timeline-api
# -> target/lambda/timeline-api/bootstrap

# Locally emulate the real AWS Lambda Runtime API and send it a test event:
cargo lambda watch -p timeline-api &
cargo lambda invoke -A '{"version":"2.0","routeKey":"GET /conversations", ...}'
```

This was run by hand during development against genuine
API-Gateway-HTTP-API-shaped events (see "What's actually been verified"
above) — it isn't part of `cargo test --workspace` because it would mean
shelling out to an external binary from the test suite.

## Building, testing, running

```
cd backend
cargo build --workspace
cargo test --workspace        # 163 tests
cargo clippy --workspace --all-targets   # should be silent
cargo fmt --all

# Run the API locally, in-memory storage only, no AWS needed:
cargo run -p timeline-api
# -> listening on http://127.0.0.1:3000
```

The local server is genuinely runnable and was exercised by hand with real
HTTP requests (`curl`) during development, not just compiled — see
"What's actually been verified" below for exactly what that covered. It
uses a throwaway RSA keypair for auth, generated fresh in memory once per
process (`timeline_api::dev_only::DEV_KEYPAIR`) — never written to disk,
never checked into git, never valid for anything real, and not a
substitute for a real Cognito user pool. An earlier version of this
checked a PEM file and JWKS into git; that was changed after review, since
committing any private key, even a throwaway one that was never valid for
anything real, trains a bad habit and trips automated secret-scanners.

## What's actually been verified, and what hasn't

Per this project's own rule against confusing "compiles"/"unit tests pass"
with "actually works": here's the honest split.

**Verified by running the actual code, not just by reading it:**
- The full local server (`cargo run -p timeline-api`), by hand, via `curl`:
  unauthenticated requests rejected (401), a garbage bearer token rejected
  (401), `POST /uploads` returning a real presigned-URL-shaped response,
  `GET /conversations` returning `[]` for a new user, and a `PATCH` then
  `GET` on a message's flags round-tripping correctly through real HTTP
  requests — all captured as committed tests in `timeline-api/tests/app.rs`
  (`tower::ServiceExt::oneshot` against the real `Router`, not a mock).
- `timeline-auth`'s Cognito access-token verification, including the
  security-relevant rejection paths (wrong `client_id`, wrong `token_use`,
  unknown signing key, wrong issuer, expired token, garbage input) — against
  a real, self-signed RSA keypair and real `jsonwebtoken` signing/verification,
  not a stub.
- The in-memory storage adapters, including that the auto/user flag
  separation from the migration plan's section 4.1 holds through the real
  public trait methods, not just by inspection.

**Also verified by running the actual code — the Lambda runtime path
specifically:**
- `cargo lambda build --release --arm64 -p timeline-api` produces a real
  ARM64 `bootstrap` binary (confirmed with `file`: `ELF 64-bit LSB pie
  executable, ARM aarch64` — genuine cross-compilation, the build host is
  x86_64). `cargo-lambda` needs [Zig](https://ziglang.org) as its
  cross-compilation linker, not Docker; Zig isn't preinstalled here and was
  fetched directly from ziglang.org (see "Installing cargo-lambda" below).
- `cargo lambda watch -p timeline-api` was run to start a local emulation of
  the real AWS Lambda Runtime API, and `cargo lambda invoke` was used to send
  genuine API-Gateway-HTTP-API-shaped events at it — not just `cargo run` +
  `curl` against a plain TCP listener, but the actual code path Lambda uses
  to hand a function its event and collect its response. Three requests were
  sent this way and all came back correct: an authenticated `GET
  /conversations` (200, `[]`), an unauthenticated request (401, `missing
  Authorization header`), a garbage-token request (401, `invalid or expired
  token`), and an authenticated `POST /uploads` (200, a real `upload_id` +
  presigned-URL-shaped response). This is a manual verification session, the
  same as the earlier `curl` one — it isn't captured as a committed,
  automated test, because doing so would mean shelling out to `cargo lambda`
  from the test suite, which is out of scope for now.

**Not verified, because there is no AWS access in this environment (no
credentials, no LocalStack, no SAM CLI):**
- `timeline-storage/src/s3.rs` and `timeline-storage/src/dynamo/*` — the
  real AWS SDK adapters compile and their pure request-building logic is
  unit-tested (see `dynamo/message_flags_table.rs`'s temporary private-function
  tests for the auto/user DynamoDB-expression separation specifically), but
  no `send()` call in either file has ever actually reached AWS or LocalStack.
- `infra/template.yaml` (the SAM template) has never been run through `sam
  validate` or `sam deploy` — no SAM CLI in this environment.
- Nothing has been verified against a real Cognito user pool's actual
  tokens — only against a self-signed test keypair standing in for one.
- Deploying the built Lambda binary to real AWS Lambda — untested; the
  binary now has confirmed local-emulator behavior (above), but that's
  `cargo-lambda`'s emulation of the Runtime API, not the real service.

This is exactly the gap the migration plan's V2 test list already expected
("also run the full suite once against real... AWS S3+DynamoDB before
calling V2 done" / "verified against a real test Cognito user pool") — it's
tracked, not hidden.

## Test coverage

`timeline-core` stays at 100% line/function/region coverage (unchanged from
V1). The new V2 crates do not, and the shortfall is concentrated exactly
where you'd expect given the paragraph above: the real S3/DynamoDB
`send()` calls in `timeline-storage`, which cannot be exercised without
live AWS or LocalStack. Everything reachable without a live AWS connection
(request/expression building, the axum app end-to-end, auth verification,
error-mapping) is tested. Measure with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (MIT/Apache-2.0):

```
cargo install cargo-llvm-cov --locked
rustup component add llvm-tools-preview
cargo llvm-cov --workspace --summary-only
```

## Structural enforcement of the auto/user flag separation

The migration plan's section 4.1 requires that automatic (heuristic/Bedrock)
flag writes and the user's own overrides can never cross-contaminate. This
is enforced at three layers, not just documented:

1. **Trait level** (`timeline-core::ports::message_flags`): `AutoFlagWriter`
   and `UserFlagWriter` are separate traits; neither has a method that could
   touch the other's data.
2. **Wiring level** (`timeline-api::state`): `AppState` exposes each
   capability as its own `FromRef` impl, so a route handler's function
   signature only ever names the one trait object it needs. The
   `PATCH .../flags` handler's source code has no `Arc<dyn AutoFlagWriter>`
   in scope at all — not "doesn't use it," genuinely not a parameter.
3. **DynamoDB level** (`timeline-storage::dynamo::message_flags_table`):
   auto and user flags live in disjoint attribute names (`auto_*`/`user_*`),
   and the UpdateExpression-building functions are unit-tested to prove
   each one only ever references its own half.

## Dev-only signing key: generated, not checked in

`timeline_api::dev_only::DEV_KEYPAIR` (and the equivalent private, per-file
generators in the test suites) is a throwaway RSA keypair generated fresh,
in memory, at process/test-binary start — never written to disk, never
checked into git. It's not a secret in any meaningful sense (never used
for anything real), but it also must never be mistaken for production
configuration — a real deployment needs a real Cognito user pool's real
JWKS, fetched from its `.well-known/jwks.json` endpoint (not built yet —
see "What's not built" below). Generating it at runtime instead of
checking in a PEM file (the original design) removes even the appearance
of a leaked credential, at a small cost: 2048-bit RSA generation in pure
Rust isn't free, adding a few seconds to the test binaries that need one.

## What's deliberately different from `timeline.html`

- **Session/day bucketing**: `sessions::build_blocks` only does gap-based
  splitting (UTC in, UTC out) — day-bucketing stays a client-side concern.
  See `timeline-core/src/sessions.rs`'s module doc and the plan's §4.3.
- **Anger detection**: a full Rust port of VADER instead of the original's
  AFINN lexicon (license reasons — see the plan's C2).
- **`created_at` is validated at parse time, not lazily at first use** — see
  `timeline-core/src/model.rs`'s module doc for the deliberate trade-off
  this makes (one bad timestamp now fails the whole upload).

## What's not built yet

- The S3-triggered upload-processing Lambda (parses the raw upload, runs
  dedup/heuristics, writes conversation summaries and auto flags). `POST
  /uploads` issues a presigned URL and a pending record, but nothing yet
  turns an uploaded file into conversations and flags.
- `GET /export` (generate and serve the annotated `conversations.json`).
- Fetching/caching a real Cognito user pool's JWKS over HTTP (`CognitoVerifier`
  takes an already-loaded `JwkSet` today — see `timeline-auth/src/lib.rs`'s
  module doc).
- `timeline.html` itself — still 100% unmodified, still using its own
  client-side JS for everything. Nothing in the browser calls this backend
  yet.
- Real-AWS/LocalStack integration tests, a real Cognito user pool, and an
  actual deployment — all blocked on AWS account access (see above).

## Test fixture provenance

`timeline-core/tests/fixtures/sample_conversations.json` is a trimmed,
format-preserving excerpt of a real Anthropic `conversations.json` export,
selected because 3 of its 6 conversations contain a genuine
resend-after-empty-assistant-reply duplicate. See the plan's C1/C8/C9.
