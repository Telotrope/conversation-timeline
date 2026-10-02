#!/usr/bin/env bash
#
# Brings up one half of the local dev environment, restarting it only when
# the thing already running is actually out of date.
#
#   scripts/dev-up.sh backend   # timeline-api on $PORT (default 3000)
#   scripts/dev-up.sh static    # timeline.html on $STATIC_PORT (default 8000)
#
# The restart-only-if-stale behavior is the point, not an optimization:
# timeline-api keeps every upload and confirmed flag in memory only, so
# restarting it when nothing changed throws away whatever you were looking
# at. See docs/plans/completed/2026-09-28-frontend-quality-of-life.md's Phase 1.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/port-control.sh
source "${REPO_ROOT}/scripts/port-control.sh"

PORT="${PORT:-3000}"
STATIC_PORT="${STATIC_PORT:-8000}"
STATE_DIR="${REPO_ROOT}/.dev-state"
# Scoped by port: the recorded hash describes the instance on one specific
# port, so a second instance elsewhere (or the test suite, which deliberately
# uses its own ports to avoid touching a real session) must not overwrite it.
HASH_FILE="${STATE_DIR}/timeline-api.${PORT}.hash"
BINARY="${REPO_ROOT}/backend/target/debug/timeline-api"

# cargo and zig are installed outside the default PATH on this machine; the
# e2e suite does the same thing for the same reason (see
# e2e/upload-flow.spec.js's beforeAll and backend/README.md's prerequisites).
# Without this, running from a VS Code task rather than a login shell fails
# to find cargo at all.
export PATH="${HOME}/.cargo/bin:${HOME}/.local/opt/zig:${PATH}"

usage() {
  echo "usage: $(basename "$0") {backend|static}" >&2
  exit 2
}

start_backend() {
  echo "Building timeline-api (no-op if nothing changed)…"
  # cargo's own incremental build is the freshness check: it already tracks
  # the whole dependency graph, including Cargo.toml changes that touch no
  # .rs file, which a hand-rolled mtime scan would miss.
  (cd "${REPO_ROOT}/backend" && cargo build -p timeline-api)

  if [ ! -x "$BINARY" ]; then
    echo "error: expected a built binary at ${BINARY}, but it is missing" >&2
    exit 1
  fi

  local current_hash recorded_hash occupant occupant_cmd
  current_hash="$(sha256sum "$BINARY" | cut -d' ' -f1)"
  recorded_hash="$(cat "$HASH_FILE" 2>/dev/null || true)"
  occupant="$(listeners_on "$PORT" | head -1)"

  if [ -n "$occupant" ]; then
    occupant_cmd="$(describe_pid "$occupant")"
    if [ "$current_hash" = "$recorded_hash" ] && [[ "$occupant_cmd" == *timeline-api* ]]; then
      echo "timeline-api already up to date (pid ${occupant}) — leaving it running."
      echo "  ${occupant_cmd}"
      exit 0
    fi
    if [ "$current_hash" != "$recorded_hash" ]; then
      echo "timeline-api on port ${PORT} is running older code; replacing it."
    else
      echo "port ${PORT} is held by something that is not timeline-api; replacing it."
    fi
    clear_port "$PORT"
  fi

  mkdir -p "$STATE_DIR"
  echo "$current_hash" > "$HASH_FILE"

  # exec the built binary directly rather than going through `cargo run`, so
  # the process holding the port is this process: whoever terminates it
  # (Ctrl+C, VS Code's task-terminate button) stops the real server, and the
  # PID that lsof reports is the one clear_port would signal.
  cd "${REPO_ROOT}/backend"
  exec "$BINARY"
}

start_static() {
  local occupant
  occupant="$(listeners_on "$STATIC_PORT" | head -1)"

  if [ -n "$occupant" ]; then
    # A static file server has no staleness to check: python3 -m http.server
    # re-reads timeline.html from disk on every request, so a long-running
    # instance can never serve older content than a fresh one would. The only
    # question is whether what is on the port actually serves the file.
    if curl -sf -o /dev/null "http://127.0.0.1:${STATIC_PORT}/timeline.html"; then
      echo "static server already serving timeline.html (pid ${occupant}) — leaving it running."
      exit 0
    fi
    echo "port ${STATIC_PORT} is held by something that does not serve timeline.html; replacing it."
    clear_port "$STATIC_PORT"
  fi

  cd "$REPO_ROOT"
  exec python3 -m http.server "$STATIC_PORT"
}

case "${1:-}" in
  backend) start_backend ;;
  static)  start_static ;;
  *)       usage ;;
esac
