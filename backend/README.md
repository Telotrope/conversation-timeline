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
cargo test --workspace        # runs all 120 tests (unit, property, snapshot, regression)
cargo clippy --workspace --all-targets   # lints; should be silent
cargo fmt --all                # reformat, if you've edited anything
```

## Tests live in `tests/`, not alongside the implementation

Every test calls only `timeline-core`'s public API — nothing reaches into a
private function directly. This is a deliberate project convention (see
CLAUDE.md's "Test only through the public API"), not the Rust default: the
usual idiom is `#[cfg(test)] mod tests` colocated in the same file as the
code it tests, which *can* access private items. That's allowed here too,
but only as temporary scaffolding while developing a specific mechanism —
once a public-API test proves the same behavior from outside, the
private-function test is removed (in a commit separate from the one that
added the replacement, so git history keeps a record of what was directly
verified). The `vader/algorithm.rs` module's private helpers (`negation_check`,
`scalar_inc_dec`, `special_idioms_check`, ...) are the main example: every
one of them is now proven through real sentences in `tests/vader_algorithm.rs`
calling `polarity_scores` — the crate's only public entry point into VADER —
rather than by calling those helpers directly.

One structural change fell out of this: `negation_check` used to take a
`start_i: usize` parameter that only ever legally took the values 0, 1, or 2,
which meant its `match` needed a `_ => ...` wildcard arm to compile — an arm
no real input could ever reach. It's now a 3-variant `Distance` enum instead,
so the impossible case is unrepresentable and the wildcard is gone. That
came directly out of trying to reach 100% coverage through public tests
alone: a line only a privileged test could reach turned out to be a sign the
*type* was wrong, not the test.

## Test coverage

Per-project requirement: 100% test coverage, using only public-API tests.
Measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov)
(MIT/Apache-2.0):

```
cargo install cargo-llvm-cov --locked   # one-time setup
rustup component add llvm-tools-preview # one-time setup
cargo llvm-cov --workspace --summary-only
```

Current result: **100.00% line, function, and region coverage across every
file** — no exceptions, and none of it reached via privileged access to a
private function.

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
