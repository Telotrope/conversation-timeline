# Automatic build cleanup

**Status:** draft, awaiting approval. Closes C3 of
[2026-10-01-smaller-debug-builds.md](completed/2026-10-01-smaller-debug-builds.md#L62).

## Why

Cargo keeps every outdated copy of every compiled program and library. On 2026-10-02 the build
folder reached 30 GB, the disk had 639 MB free, and you had to step in. The smaller-builds plan
logged this risk (its C3) with a 15 GB trigger, and no session acted on it.

That day, by hand, I asked Cargo which files its current builds use, and deleted the rest of
`target/debug`. It freed 14.6 GB with no library rebuilt
([analysis](../analysis/2026-10-02-page-hosting-deployment.md)). This plan makes that run by
itself, safely, with nobody watching.

## Reuse check

- **`cargo-sweep`** (a Cargo plugin): decides what's old by when a file was last read. This disk
  is mounted `relatime` (checked with `mount`), which records reads at most once a day. So
  "unused today" can't be told from "used an hour ago", and today's 15 GB of buildup would
  survive. Rejected on that mechanism; I didn't check its license.
- **`cargo clean`**: deletes everything and forces a full rebuild (about 3.3 GB, several
  minutes, in whichever session builds next). You said no.
- **What I did by hand on 2026-10-02**: Cargo's own JSON build report lists every file the current
  builds use. That's exact, and nothing current is rebuilt. Kept, with today's bug fixed (§1,
  step 3).

## Design

### §1. `scripts/clean-build.py` (Python 3, standard library only)

1. Note the start time.
2. Ask Cargo for the current builds, with `--message-format=json`, in `backend/`:
   - `cargo test --workspace --no-run` (what tests use);
   - `cargo build --workspace --bins --examples`;
   - `cargo build -p timeline-api` (what the browser tests and `dev-up.sh` run).

   If the sources changed since the last build, this compiles them, as any test run would. **If
   any command fails** (for example, another session is midway through an edit that doesn't
   compile), stop, log Cargo's error and delete nothing.
3. **Keep:** every file and folder whose 16-character build identifier appears in those reports.
   Also keep the copies in `deps/` that are the same file (same inode, Linux's file number) as
   the unhashed programs Cargo reports, like `target/debug/timeline-api`. On 2026-10-02 the hand
   run missed those and 10 programs had to be relinked.
4. **Take Cargo's own lock** on the build folder, so no build runs while files are deleted. Which
   file Cargo locks is to be confirmed in Cargo's source during implementation (C2). If another
   build holds it for more than 5 minutes, skip this run and log that.
5. **Delete:** entries in `target/debug/deps`, `.fingerprint` and `build` that aren't kept *and*
   are older than the start time. Anything a parallel build made after step 1 is never touched.
   In `target/debug/incremental` (Cargo's saved partial work, named differently), keep each
   crate's newest folders, as many as that crate has current builds, and delete older ones.
   Misjudging one costs that crate one slower compile, not a rebuild.
6. Release the lock and log what was freed to `backend/target/clean-build.log`.

Options:
- `--if-larger-than <size>`: exit at once, without running Cargo, when `backend/target` is
  smaller. The default for automatic runs is **12 GB**: a clean build is 3.3 GB, and after the
  hand run the folder was 14 GB including 4.7 GB of coverage builds this doesn't touch.
- `--dry-run`: report only.

### §2. Running by itself

A Claude Code **`Stop` hook** in the project's [.claude/settings.json](../../.claude/settings.json):
after any session's turn ends, it runs `scripts/clean-build.py --if-larger-than 12G` in the
background, at low priority (`nice`). Under 12 GB the check takes about a second and does
nothing. This covers every session, whatever it built, with no step for you. The hook is set up
with the `update-config` skill.

### §3. Not in this plan

- `target/llvm-cov-target` (4.7 GB, coverage builds) and `target/aarch64-unknown-linux-gnu`
  (816 MB, the Lambda build) have their own build settings, so listing them needs their own
  commands. See C3.

## Tests

`scripts/test-clean-build.sh`, run manually like the other script tests, against **real Cargo**
on a small scratch Cargo project made in a temporary folder (a library, a binary, a test):

- Build, change the source, build again: the outdated copies are deleted, and a following
  `cargo build` and `cargo test --no-run` report every item fresh, including the binary.
- A file newer than the start time is kept, whether or not Cargo listed it.
- A source that doesn't compile: exit with an error naming Cargo's message, nothing deleted.
- The lock held by another process past the wait (shortened by an option in the test): skips and
  logs, nothing deleted.
- Under `--if-larger-than`: no Cargo command is run.
- `--dry-run` deletes nothing and reports what it would.

Then one real run on this repository's build folder, recording before and after sizes and the
freshness check in the commit message.

## Self-critique log

### C1 [RESOLVED]: A cleanup racing another session's build could break that build
Original concern: sessions build in parallel. A file deleted mid-build fails that build.
**Resolution:** deletions happen under Cargo's own lock, and only for files older than the
cleanup's start ([§1 steps 4-5 (line 46)](2026-10-02-automatic-build-cleanup.md#L46)).

### C2 [OPEN]: Which file Cargo locks is not yet confirmed
I believe Cargo locks `target/debug/.cargo-lock`, but haven't read that in Cargo's source.
**Mitigation in plan:** the start-time rule in step 5 alone already protects new files.
**Open:** confirm it in Cargo's source during implementation, and test that a held lock makes
the cleanup wait. Trigger: implementation.

### C3 [OPEN]: The coverage and Lambda build folders still grow
Together they are 5.5 GB today. **Open:** extend the listing to them with their own commands.
Trigger: the log shows `backend/target` over 12 GB right after a cleanup.

### C4 [RESOLVED]: The hook could compile code after every turn
Original concern: step 2 compiles whatever changed. **Resolution:** the size check runs first,
so the compile only happens once the folder passes 12 GB, at low priority and in the background
([§2 (line 62)](2026-10-02-automatic-build-cleanup.md#L62)).
