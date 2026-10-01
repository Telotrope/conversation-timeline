# Smaller debug builds: keep only line numbers in debugging information

**Status:** done 2026-10-01; numbers in the commit and in backend/README.md.

## Why

A clean build of [backend/](../../backend/) takes 7.3 GB, and Cargo keeps every outdated copy, so the
build folder reached 33 GB and filled the disk three times on 2026-10-01. Measured that day with
`readelf`, most of each test program is debugging information (the data a debugger uses to step
through code and show variable values):

| Test program | Debugging information | The program itself |
|---|---|---|
| `memory_uploads` | 233 MB | 6 MB |
| `s3_object_store` | 284 MB | 66 MB |
| `flags_caps` | 24 MB | 8 MB |

Every program built on `timeline-storage` or `timeline-api` carries the AWS SDK's debugging
information, even when it never calls AWS. The user agreed on 2026-10-01 to give up step-through
debugging with variable values.

## Change

Add to [backend/Cargo.toml](../../backend/Cargo.toml):

```toml
[profile.dev]
debug = "line-tables-only"
```

`line-tables-only` is a standard Cargo setting (stable since Rust 1.71; this machine has 1.98.1).
Programs keep the information that maps machine code to file and line, so crash backtraces still
name `file.rs:line`. What goes is the information a debugger needs to show variables and types.
It applies to the `dev` profile, the one `cargo build`, `cargo test` and `cargo run` use. `cargo
llvm-cov` measures coverage with separate instrumentation, not debugging information, so coverage
should be unaffected. Step 4 below checks that rather than assuming it.

No code changes. The release build used for Lambda is not affected.

## Steps

1. **Measure before.** Run `cargo clean`, then `cargo test --workspace`, and record `du -sh
   target` and the sizes of `memory_uploads`, `s3_object_store` and `flags_caps`.
2. **Make the change** above.
3. **Measure after.** Repeat step 1 exactly and record the same numbers.
4. **Check nothing else changed.**
   - All suites pass: `cargo test --workspace`, the frontend unit tests, the browser tests.
   - `cargo llvm-cov -p timeline-storage --summary-only` gives the same per-file coverage as on
     2026-10-01 (`s3.rs` 100%, `conversations_table.rs` 98.41%, `message_flags_table.rs` 99.20%).
   - A deliberately failing test run with `RUST_BACKTRACE=1` still shows file and line in the
     backtrace. This is a throwaway check on a scratch copy, never committed.
5. **Report** the before and after numbers in the commit message and in
   [backend/README.md](../../backend/README.md), next to the build instructions.

## Not in this plan

**Combining each crate's test files into one program.** That would also save space, but it moves
committed tests and costs some isolation between test files. Once debugging information is
trimmed, the saving should be small. Decide after step 3's measurements.

## Self-critique log

### C1 [RESOLVED]: The saving is predicted, not measured
The table above measures today's programs; how much `line-tables-only` removes isn't known yet.
**Resolution:** steps 1 and 3 measure before and after on a clean build, and step 5 reports the
real numbers. See [Steps (line 40)](2026-10-01-smaller-debug-builds.md#L40).

### C2 [RESOLVED]: Coverage measurement could quietly change
If the setting affected coverage, the 2026-10-01 numbers would shift without anyone noticing.
**Resolution:** step 4 compares per-file coverage with the recorded numbers. See
[step 4 (line 46)](2026-10-01-smaller-debug-builds.md#L46).

### C3 [OPEN]: Old copies still pile up
This shrinks each build but doesn't stop Cargo keeping outdated copies; the folder will still grow
with every change. **Mitigation in plan:** each copy becomes much smaller. **Open:** trigger is the
build folder passing 15 GB again, at which point a cleanup routine is worth planning.
