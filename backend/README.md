# timeline-core (V1)

Pure Rust port of the non-presentation logic from `timeline.html`, per
[docs/plans/2026-09-09-rust-aws-backend-migration.md](../docs/plans/2026-09-09-rust-aws-backend-migration.md).
This is **V1** of that plan: a tested library crate with no I/O, no AWS SDK,
no HTTP server yet — that starts in V2. Right now there is nothing to deploy
or run as a service; this is code you build and test.

## What's in this crate

- `dedup` — removes retried duplicate human messages (port of `dedupChatMessages`).
- `format` — accepts a raw export or an already-processed file (port of `unwrapUploadedJSON`).
- `sessions` — splits a conversation into session blocks on 15-minute-plus gaps (port of `buildBlocks`, gap-only — see the module doc for what's deliberately different from the original and why).
- `flags::caps` — ALL-CAPS emphasis detection (dictionary-based).
- `flags::criticism` — criticism-of-Claude keyword detection.
- `flags::anger` — anger detection, now backed by a Rust port of VADER instead of AFINN (license reasons — see the plan's C2).
- `flags::matrix` — the four-visibility-state auto/user flag logic.
- `vader` — the VADER sentiment algorithm itself (lexicon, boosters, negation, punctuation emphasis, compound score), used by `flags::anger`.

## Prerequisites

You need the Rust toolchain (`rustc`/`cargo`, via [rustup](https://rustup.rs))
**and** a C linker — `rustup` does not install one. On Debian/Ubuntu:

```
sudo apt install build-essential
```

## Building and testing

```
cd backend
cargo build --workspace       # compiles the crate
cargo test --workspace        # runs all 111 tests (unit, property, snapshot, regression)
cargo clippy --workspace --all-targets   # lints; should be silent
cargo fmt --all                # reformat, if you've edited anything
```

## Test coverage

Per-project requirement: 100% test coverage. Measured with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) (MIT/Apache-2.0):

```
cargo install cargo-llvm-cov --locked   # one-time setup
rustup component add llvm-tools-preview # one-time setup
cargo llvm-cov --workspace --summary-only
```

Current result: **100.00% line coverage and 100.00% function coverage across
every file.** Region coverage (a finer-grained metric that also counts each
side of short-circuit boolean operators and macro-internal branches
separately) sits at 99.65% — the 8 remaining "missed regions" are all in
`format.rs` and are the synthetic negative arm the `matches!(err, ExpectedVariant)`
macro generates internally for each test assertion that confirms an error is
a *specific* variant; since every such test already establishes which variant
it is, that generated arm can never be taken in that test. It isn't a gap in
tested production logic (line and function coverage there are both 100%,
including the `Display` and `source()` implementations for every variant) —
closing it would mean writing tests that assert an error *isn't* some other
variant, which doesn't verify any additional behavior. Flagging this
explicitly rather than silently rounding "very close to 100%" up to it.

## What's deliberately different from `timeline.html`

- **Session/day bucketing**: `sessions::build_blocks` only does gap-based
  splitting (UTC in, UTC out). The original `buildBlocks` also bucketed by
  the viewer's local calendar day before splitting — that part stays a
  client-side concern in later versions, since a backend has no idea what
  timezone the viewer is in. See the module doc in `src/sessions.rs` and the
  plan's §4.3 for why, including the real bug this avoids reintroducing.
- **Anger detection**: uses a full Rust port of VADER instead of the
  original's AFINN lexicon (AFINN is ODbL-licensed, not on this project's
  approved license list). The anger-specific phrase list and
  exclamation-mark-burst logic are unchanged; only the underlying sentiment
  score changed, along with its threshold (recalibrated — VADER's compound
  score is normalized to `[-1, 1]`, AFINN's wasn't). See `src/flags/anger.rs`.

## Test fixture provenance

`tests/fixtures/sample_conversations.json` is a trimmed, format-preserving
excerpt of a real Anthropic `conversations.json` export you supplied,
selected specifically because 3 of its 6 conversations contain a genuine
resend-after-empty-assistant-reply duplicate. See the plan's C1/C8/C9 for
how it was built and a real discrepancy it surfaced against
`timeline-project-decisions.md`'s dedup statistic.

## Not yet built (later versions per the plan)

No server, no AWS resources, no Bedrock, no payments, no persistence. Those
start at V2. This crate isn't runnable as anything other than a tested
library right now.
