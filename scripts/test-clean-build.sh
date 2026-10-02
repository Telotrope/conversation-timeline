#!/usr/bin/env bash
#
# Tests clean-build.py against real Cargo, on a small scratch Cargo project
# (a library, a program and a test) built in a temporary folder. Run manually:
#
#   scripts/test-clean-build.sh
#
# docs/plans/2026-10-02-automatic-build-cleanup.md, "Tests".

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLEAN="${CLEAN:-$REPO_ROOT/scripts/clean-build.py}"
export PATH="$HOME/.cargo/bin:$PATH"
work="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$work"' EXIT

failures=0
pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1" >&2; failures=$((failures + 1)); }

proj="$work/scratch"
mkdir -p "$proj/src" "$proj/tests" "$proj/examples"
# $1 is both the answer and the patch version: a new version gives every
# build a new identifier, leaving the old files behind as outdated copies, as
# a dependency or feature change does in the real workspace.
write_source() {
  printf '[package]\nname = "scratch"\nversion = "0.1.%s"\nedition = "2021"\n[features]\nextra = []\n' "$1" > "$proj/Cargo.toml"
  echo "pub fn answer() -> u32 { $1 }" > "$proj/src/lib.rs"
  echo 'fn main() { println!("{}", scratch::answer()); }' > "$proj/src/main.rs"
  echo '#[test] fn answers() { assert!(scratch::answer() > 0); }' > "$proj/tests/t.rs"
  echo 'fn main() { println!("{}", scratch::answer() + 1); }' > "$proj/examples/e.rs"
}
# The program is built twice with different features, as this repository
# builds timeline-api (whole workspace, and alone): two current copies, but
# target/debug/scratch is the same file as only the last one built.
LISTINGS=(--listing "test --no-run" --listing "build --bins --examples" --listing "build --bins --features extra")
clean() { python3 "$CLEAN" --workspace "$proj" "${LISTINGS[@]}" "$@"; }
build() { (cd "$proj" && cargo test --no-run -q 2>/dev/null && cargo build --bins --examples -q 2>/dev/null \
  && cargo build --bins --features extra -q 2>/dev/null); }
deps_count() { find "$proj/target/debug/deps" "$proj/target/debug/examples" -maxdepth 1 \( -name 'scratch-*' -o -name 'libscratch-*' -o -name 'e-*' \) | wc -l; }
# Every build Cargo reports for the listings is fresh: nothing rebuilt.
all_fresh() {
  (cd "$proj" && { cargo test --no-run --message-format=json 2>/dev/null; cargo build --bins --examples --message-format=json 2>/dev/null;
                   cargo build --bins --features extra --message-format=json 2>/dev/null; }) \
    | python3 -c '
import json, sys
stale = [m["target"]["name"] for m in map(json.loads, sys.stdin)
         if m.get("reason") == "compiler-artifact" and not m["fresh"]]
sys.exit(f"rebuilt: {stale}" if stale else 0)'
}

echo "clean-build.py"
write_source 1; build
write_source 2; build
write_source 3; build
before="$(deps_count)"

# An unlisted file older than the run, and one dated after it starts.
old="$proj/target/debug/deps/libjunk-0123456789abcdef.rlib"
new="$proj/target/debug/deps/libjunk-fedcba9876543210.rlib"
touch -d '2 hours ago' "$old"; touch -d '+1 hour' "$new"

if clean --dry-run > "$work/out.txt"; then
  grep -q "dry run: would delete" "$work/out.txt" && [ "$(deps_count)" = "$before" ] && [ -e "$old" ] \
    && pass "a dry run reports and deletes nothing" || fail "dry run: $(cat "$work/out.txt")"
else
  fail "dry run failed: $(cat "$work/out.txt")"
fi

if clean > "$work/out.txt"; then
  pass "succeeds on a project with outdated copies"
else
  fail "failed: $(cat "$work/out.txt")"
fi
after="$(deps_count)"
[ "$after" -lt "$before" ] && pass "deletes outdated copies ($before -> $after entries)" \
  || fail "deleted nothing: $before -> $after"
all_fresh 2> "$work/fresh.txt" && pass "every current build stays fresh: both builds of the program, and the example" \
  || fail "$(cat "$work/fresh.txt")"
[ ! -e "$old" ] && pass "deletes an unlisted file older than the run" || fail "kept $old"
[ -e "$new" ] && pass "keeps a file newer than the run's start" || fail "deleted $new"
grep -q "deleted .* outdated entries" "$proj/target/clean-build.log" && pass "logs what it freed" \
  || fail "log: $(cat "$proj/target/clean-build.log")"

# A source that doesn't compile: Cargo's report is incomplete, so nothing goes.
write_source 3; echo 'pub fn answer() -> u32 { not valid }' > "$proj/src/lib.rs"
touch -d '2 hours ago' "$old"
if clean > "$work/out.txt" 2>&1; then
  fail "succeeded although the source doesn't compile"
else
  [ "$?" = 1 ] || true
  grep -q "nothing deleted: .*failed" "$work/out.txt" && pass "a failed build: exits with Cargo's error" \
    || fail "wrong message: $(cat "$work/out.txt")"
fi
[ -e "$old" ] && pass "a failed build: deletes nothing" || fail "deleted $old although the build failed"
write_source 3; build

# Cargo's locks on the build folder. First confirm Cargo itself waits on each
# of the three files the cleanup takes (the plan's C2).
for name in .cargo-lock .cargo-artifact-lock .cargo-build-lock; do
  flock "$proj/target/debug/$name" sleep 30 & holder=$!
  sleep 1
  (cd "$proj" && timeout 5 cargo build --bins 2> "$work/cargo.txt" || true)
  kill "$holder"; wait "$holder" 2>/dev/null || true
  grep -q "Blocking waiting for file lock" "$work/cargo.txt" \
    && pass "Cargo waits while $name is held" \
    || fail "Cargo didn't wait on $name: $(cat "$work/cargo.txt")"
done

# Then a lock taken by "another build" between the cleanup's listing and its
# deletion: a stand-in cargo runs the real one, and after the last listing
# (`build --bins`) starts a holder of one lock and returns once it holds it.
real_cargo="$(command -v cargo)"
real_home="$HOME"
mkdir -p "$work/wrap"
cat > "$work/wrap/cargo" <<STUB
#!/usr/bin/env bash
"$real_cargo" "\$@"; status=\$?
if [ "\$*" = "build --bins --features extra --message-format=json" ]; then
  rm -f "$work/held"
  flock "\$HOLD_LOCK" sh -c 'touch "$work/held"; sleep 20' >/dev/null 2>&1 &
  while [ ! -e "$work/held" ]; do sleep 0.05; done
fi
exit \$status
STUB
chmod +x "$work/wrap/cargo"
touch -d '2 hours ago' "$old"
status=0
# HOME points away so the script's ~/.cargo/bin doesn't come before the
# stand-in; the toolchain's own folders are given explicitly.
HOLD_LOCK="$proj/target/debug/.cargo-build-lock" PATH="$work/wrap:$PATH" HOME="$work" \
  RUSTUP_HOME="$real_home/.rustup" CARGO_HOME="$real_home/.cargo" \
  python3 "$CLEAN" --workspace "$proj" "${LISTINGS[@]}" --lock-wait 2 > "$work/out.txt" 2>&1 || status=$?
pkill -f "sh -c touch $work/held" 2>/dev/null || true
if [ "$status" = 3 ] && grep -q "stayed held" "$work/out.txt" && [ -e "$old" ]; then
  pass "a Cargo lock held past the wait: exits 3, deletes nothing"
else
  fail "lock held: status $status, $(cat "$work/out.txt")"
fi
sleep 0.5

# Two cleanups at once: the second leaves it to the first.
exec 9>"$proj/target/.clean-build.lock"; flock 9
if clean > "$work/out.txt" 2>&1 && grep -q "another cleanup is running" "$work/out.txt" && [ -e "$old" ]; then
  pass "a second cleanup leaves it to the running one"
else
  fail "second cleanup: $(cat "$work/out.txt")"
fi
flock -u 9; exec 9>&-

# Under the size limit: no Cargo command runs at all.
mkdir -p "$work/bin"; printf '#!/bin/sh\necho "cargo was run" >&2\nexit 99\n' > "$work/bin/cargo"; chmod +x "$work/bin/cargo"
if HOME="$work" PATH="$work/bin:$PATH" python3 "$CLEAN" --workspace "$proj" --if-larger-than 1T > "$work/out.txt" 2>&1; then
  [ ! -s "$work/out.txt" ] && pass "under the size limit: runs no Cargo command, prints nothing" \
    || fail "under the limit printed: $(cat "$work/out.txt")"
else
  fail "under the limit failed: $(cat "$work/out.txt")"
fi
if HOME="$work" PATH="$work/bin:$PATH" python3 "$CLEAN" --workspace "$proj" --if-larger-than 1K > "$work/out.txt" 2>&1; then
  fail "over the limit, with a failing cargo, it succeeded"
else
  grep -q "over 0.0 GB: cleaning" "$work/out.txt" && grep -q "cargo was run" "$work/out.txt" \
    && pass "over the size limit: cleans" || fail "over the limit: $(cat "$work/out.txt")"
fi

echo
if [ "$failures" -eq 0 ]; then echo "All passed."; else echo "$failures failed." >&2; exit 1; fi
