#!/usr/bin/env bash
#
# Integration test for dev-up.sh / dev-down.sh. Run manually:
#
#   scripts/test-dev-up.sh
#
# Not part of `cargo test` -- it manages real OS processes on real ports,
# which the Rust suite deliberately never does.
#
# Runs on ports 3999/8999, NOT the 3000/8000 defaults, so that running the
# test never disturbs a real dev session (stopping timeline-api discards its
# in-memory uploads and flags -- a test that did that to you while proving
# restarts are safe would be self-defeating).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/port-control.sh
source "${REPO_ROOT}/scripts/port-control.sh"

export PORT=3999
export STATIC_PORT=8999
HASH_FILE="${REPO_ROOT}/.dev-state/timeline-api.${PORT}.hash"

failures=0
scratch_pids=()

pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1" >&2; failures=$((failures + 1)); }

cleanup() {
  echo
  echo "Tearing down…"
  for pid in "${scratch_pids[@]:-}"; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done
  "${REPO_ROOT}/scripts/dev-down.sh" all >/dev/null 2>&1 || true
  rm -f "$HASH_FILE"
}
trap cleanup EXIT

# Waits for a URL to return a successful (2xx) response.
wait_for_ok() {
  local url="$1" deadline=$((SECONDS + 90))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if curl -sf -o /dev/null "$url"; then return 0; fi
    sleep 0.3
  done
  return 1
}

# Waits until timeline-api specifically is the process holding a port.
#
# "Did anything answer over HTTP?" is NOT a usable readiness check here: the
# squatter case 1 parks on the port is itself an HTTP server, so it answers
# instantly and would make the test believe the backend had started before
# dev-up.sh had even replaced it. Identifying the listening process is the
# only check that actually distinguishes the two.
wait_for_backend() {
  local port="$1" deadline=$((SECONDS + 120)) pid
  while [ "$SECONDS" -lt "$deadline" ]; do
    pid="$(listeners_on "$port" | head -1)"
    if [ -n "$pid" ] && [[ "$(describe_pid "$pid")" == *timeline-api* ]]; then
      # Holding the port is not the same as being ready to answer on it.
      # timeline-api's routes are auth-gated, so an unauthenticated
      # /conversations correctly answers 401; plain `curl -s` succeeds on any
      # HTTP reply, while `curl -f` would treat that 401 as a failure.
      if curl -s -o /dev/null "http://127.0.0.1:${port}/conversations"; then
        return 0
      fi
    fi
    sleep 0.3
  done
  return 1
}

# Runs dev-up.sh where the expected outcome is "decide nothing needs doing
# and exit". If that expectation is wrong, dev-up.sh execs a long-running
# server and never returns -- inside a command substitution that would hang
# the whole suite instead of failing it. The timeout converts that hang into
# an ordinary assertion failure, which is what a test should do.
run_expecting_noop() {
  timeout 30 "${REPO_ROOT}/scripts/dev-up.sh" "$@" 2>&1 || {
    local status=$?
    if [ "$status" -eq 124 ]; then
      echo "TIMED OUT: dev-up.sh $* did not return; it started a server when it should not have"
    else
      echo "dev-up.sh $* exited with status ${status}"
    fi
  }
}

listener_pid() { listeners_on "$1" | head -1; }
# A process's start time. Combined with an unchanged PID this is what proves
# a server was never restarted: a killed-and-relaunched process would have to
# both reuse its exact PID and claim the same start time to fool this.
start_time_of() { ps -p "$1" -o lstart= 2>/dev/null || echo "gone"; }

echo "=== Setup: clean slate on ports ${PORT}/${STATIC_PORT} ==="
"${REPO_ROOT}/scripts/dev-down.sh" all >/dev/null 2>&1 || true
rm -f "$HASH_FILE"

echo
echo "=== Case 1: a wrong/stale process squatting on the port gets replaced ==="
python3 -m http.server "$PORT" >/dev/null 2>&1 &
scratch_pids+=($!)
sleep 1
squatter="$(listener_pid "$PORT")"
if [ -z "$squatter" ]; then
  fail "could not park a squatter on port ${PORT}; the rest of case 1 is meaningless"
else
  "${REPO_ROOT}/scripts/dev-up.sh" backend >/dev/null 2>&1 &
  if wait_for_backend "$PORT"; then
    backend_pid="$(listener_pid "$PORT")"
    if [ "$backend_pid" = "$squatter" ]; then
      fail "the squatter (pid ${squatter}) is still holding the port"
    else
      pass "squatter replaced (pid ${squatter} -> ${backend_pid})"
    fi
    if [[ "$(describe_pid "$backend_pid")" == *timeline-api* ]]; then
      pass "the process on the port is timeline-api"
    else
      fail "expected timeline-api on the port, found: $(describe_pid "$backend_pid")"
    fi
    if [ "$(listeners_on "$PORT" | wc -l)" -eq 1 ]; then
      pass "exactly one process is listening"
    else
      fail "expected exactly one listener, found $(listeners_on "$PORT" | wc -l)"
    fi
  else
    fail "timeline-api never answered on port ${PORT}"
  fi
fi

echo
echo "=== Case 2: an already-current backend is left completely alone ==="
# The property under test: rerunning must not restart a server that does not
# need it, because that would silently discard its in-memory uploads and flags.
before_pid="$(listener_pid "$PORT")"
before_start="$(start_time_of "$before_pid")"
if [ -z "$before_pid" ]; then
  fail "no backend running; case 2 cannot run"
else
  rerun_output="$(run_expecting_noop backend)"
  after_pid="$(listener_pid "$PORT")"
  after_start="$(start_time_of "$after_pid")"

  if [ "$before_pid" = "$after_pid" ]; then
    pass "PID unchanged (${before_pid}) — the server was not restarted"
  else
    fail "PID changed ${before_pid} -> ${after_pid}; the server was needlessly restarted"
  fi
  if [ "$before_start" = "$after_start" ]; then
    pass "process start time unchanged — same process, not a same-PID coincidence"
  else
    fail "start time changed (${before_start} -> ${after_start})"
  fi
  if grep -q "already up to date" <<<"$rerun_output"; then
    pass "reported 'already up to date'"
  else
    fail "expected an 'already up to date' message, got: ${rerun_output}"
  fi
fi

echo
echo "=== Case 3: a backend running older code IS replaced ==="
# Staleness is simulated by corrupting the recorded hash rather than editing
# Rust source. This exercises the same comparison and the same branch, while
# keeping the test from mutating tracked files (which a crash mid-run would
# leave behind) and from depending on whether a source edit reliably changes
# the compiled binary's bytes.
stale_pid="$(listener_pid "$PORT")"
echo "0000000000000000000000000000000000000000000000000000000000000000" > "$HASH_FILE"
"${REPO_ROOT}/scripts/dev-up.sh" backend >/dev/null 2>&1 &
# Waiting for "a timeline-api holds the port" is not enough here: one already
# does, so that condition is true before the replacement has happened at all.
# The property to wait for is that the holder has *changed*.
replace_deadline=$((SECONDS + 120))
while [ "$SECONDS" -lt "$replace_deadline" ]; do
  [ "$(listener_pid "$PORT")" != "$stale_pid" ] && break
  sleep 0.3
done
if wait_for_backend "$PORT"; then
  new_pid="$(listener_pid "$PORT")"
  if [ "$new_pid" != "$stale_pid" ]; then
    pass "stale backend replaced (pid ${stale_pid} -> ${new_pid})"
  else
    fail "stale backend was NOT replaced; pid is still ${stale_pid}"
  fi
  if [ "$(cat "$HASH_FILE")" != "0000000000000000000000000000000000000000000000000000000000000000" ]; then
    pass "recorded hash was refreshed"
  else
    fail "recorded hash still holds the bogus value"
  fi
else
  fail "no backend answering after the stale-code restart"
fi

echo
echo "=== Case 4: the static server is liveness-only, never restarted for freshness ==="
"${REPO_ROOT}/scripts/dev-up.sh" static >/dev/null 2>&1 &
if wait_for_ok "http://127.0.0.1:${STATIC_PORT}/timeline.html"; then
  pass "static server came up and serves timeline.html"
  static_before="$(listener_pid "$STATIC_PORT")"
  static_before_start="$(start_time_of "$static_before")"
  static_output="$(run_expecting_noop static)"
  static_after="$(listener_pid "$STATIC_PORT")"
  static_after_start="$(start_time_of "$static_after")"

  if [ "$static_before" = "$static_after" ] && [ "$static_before_start" = "$static_after_start" ]; then
    pass "static server left alone on rerun (pid ${static_before})"
  else
    fail "static server was restarted (${static_before} -> ${static_after})"
  fi
  if grep -q "already serving" <<<"$static_output"; then
    pass "reported 'already serving'"
  else
    fail "expected an 'already serving' message, got: ${static_output}"
  fi
else
  fail "static server never answered on port ${STATIC_PORT}"
fi

echo
echo "=== Case 5: dev-down.sh stops things unconditionally ==="
"${REPO_ROOT}/scripts/dev-down.sh" all >/dev/null 2>&1
sleep 0.5
if [ -z "$(listeners_on "$PORT")" ]; then
  pass "backend port ${PORT} is clear"
else
  fail "something is still listening on ${PORT}"
fi
if [ -z "$(listeners_on "$STATIC_PORT")" ]; then
  pass "static port ${STATIC_PORT} is clear"
else
  fail "something is still listening on ${STATIC_PORT}"
fi
if [ ! -f "$HASH_FILE" ]; then
  pass "recorded hash removed, so the next start is not mistaken for up-to-date"
else
  fail "recorded hash file survived dev-down"
fi

echo
echo "=== Case 6: 'all' starts the page and the backend together ==="
all_output="${REPO_ROOT}/.dev-state/test-all.out"
"${REPO_ROOT}/scripts/dev-up.sh" all > "$all_output" 2>&1 &
if wait_for_backend "$PORT"; then
  pass "the backend answers on ${PORT}"
else
  fail "no backend after 'all': $(cat "$all_output")"
fi
if curl -sf -o /dev/null "http://127.0.0.1:${STATIC_PORT}/timeline.html"; then
  pass "the page is served on ${STATIC_PORT}"
else
  fail "no page on ${STATIC_PORT} after 'all'"
fi
if grep -q "Page: http://localhost:${STATIC_PORT}/timeline.html" "$all_output"; then
  pass "prints the page's address"
else
  fail "no page address printed: $(cat "$all_output")"
fi
rm -f "$all_output"
"${REPO_ROOT}/scripts/dev-down.sh" all >/dev/null 2>&1

echo
if [ "$failures" -eq 0 ]; then
  echo "All checks passed."
else
  echo "${failures} check(s) failed." >&2
fi
exit "$failures"
