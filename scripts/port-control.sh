# Finding and clearing whatever holds one of this project's dev ports.
# Sourced by dev-up.sh and dev-down.sh; not executable on its own.

# Prints the PIDs listening on a TCP port, one per line, or nothing.
# lsof exits non-zero when there are no matches, which is a normal "nobody
# is there" answer rather than a failure, so that case is folded into an
# empty result instead of aborting under `set -e`.
listeners_on() {
  local port="$1"
  lsof -ti "tcp:${port}" 2>/dev/null || true
}

# Prints a process's full command line, for showing the user what is about
# to be signalled. A process that exited between the lsof call and this one
# is reported as such rather than printing an empty string.
describe_pid() {
  local pid="$1"
  ps -p "$pid" -o cmd= 2>/dev/null || echo "(process $pid already gone)"
}

# Terminates everything listening on a port: SIGTERM, up to 3s of grace,
# then SIGKILL for anything still holding on. Every process signalled is
# announced with its PID and command line first -- this is the one place
# the dev scripts can destroy work (timeline-api's storage is in-memory),
# so it never happens silently.
clear_port() {
  local port="$1"
  local pids pid
  pids="$(listeners_on "$port")"
  if [ -z "$pids" ]; then
    return 0
  fi

  for pid in $pids; do
    echo "  stopping pid ${pid}: $(describe_pid "$pid")"
    # A failure here almost always means the process exited on its own
    # between listing and signalling. Report it rather than discarding it,
    # so a genuine permissions problem is not mistaken for that.
    kill "$pid" 2>/dev/null || echo "  (pid ${pid} did not accept SIGTERM; it may have already exited)"
  done

  local waited=0
  while [ "$waited" -lt 30 ]; do
    if [ -z "$(listeners_on "$port")" ]; then
      return 0
    fi
    sleep 0.1
    waited=$((waited + 1))
  done

  for pid in $(listeners_on "$port"); do
    echo "  pid ${pid} still holding port ${port} after 3s; sending SIGKILL"
    kill -9 "$pid" 2>/dev/null || echo "  (pid ${pid} did not accept SIGKILL; it may have already exited)"
  done
}
