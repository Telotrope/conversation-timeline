#!/usr/bin/env bash
#
# Stops one or both halves of the local dev environment.
#
#   scripts/dev-down.sh backend | static | all
#
# Unconditional, unlike dev-up.sh: "down" always means down, with no
# freshness check. Stopping timeline-api discards its in-memory uploads and
# flags, which is what you asked for by running this.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=scripts/port-control.sh
source "${REPO_ROOT}/scripts/port-control.sh"

PORT="${PORT:-3000}"
STATIC_PORT="${STATIC_PORT:-8000}"
HASH_FILE="${REPO_ROOT}/.dev-state/timeline-api.${PORT}.hash"

stop_backend() {
  if [ -z "$(listeners_on "$PORT")" ]; then
    echo "nothing listening on port ${PORT}."
  else
    echo "stopping whatever holds port ${PORT}:"
    clear_port "$PORT"
  fi
  # The recorded hash describes a running instance. With none running it is
  # stale, and leaving it would let a later dev-up.sh mistake a fresh start
  # for an up-to-date one.
  rm -f "$HASH_FILE"
}

stop_static() {
  if [ -z "$(listeners_on "$STATIC_PORT")" ]; then
    echo "nothing listening on port ${STATIC_PORT}."
  else
    echo "stopping whatever holds port ${STATIC_PORT}:"
    clear_port "$STATIC_PORT"
  fi
}

case "${1:-}" in
  backend) stop_backend ;;
  static)  stop_static ;;
  all)     stop_backend; stop_static ;;
  *)       echo "usage: $(basename "$0") {backend|static|all}" >&2; exit 2 ;;
esac
