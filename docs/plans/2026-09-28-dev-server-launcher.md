# Dev server launcher: reliable restart + a VS Code task button

## Problem

Today, running the local dev environment means two manual terminal commands (per the root
[README.md](../../README.md#running-this-locally)):

```
cd backend && cargo run -p timeline-api
cd .. && python3 -m http.server 8000
```

Two concrete pain points the user named, prompted directly by this session's back-and-forth
(restarting the backend after code changes, and wanting to avoid the terminal in VS Code Web):

1. **Restarts are unreliable.** If a previous `cargo run -p timeline-api` is still bound to
   port 3000 (left over from an earlier terminal, a crashed shell, an old VS Code task, etc.), a
   fresh `cargo run` either fails to bind or the user can't easily tell which process — old code
   or new — is actually answering requests on that port. There is no step today that guarantees
   "the process now listening is the one I just started."
2. **Terminal use inside VS Code Web is described as hacky on some platforms.** The user wants a
   one-click way to bring the environment up without hand-typing shell commands.

## Scope clarification: what "ensure the http server is connecting to timeline-api" means here

There is no persistent connection between the two server processes to verify — `timeline.html` is
a static file; the only thing that talks to `timeline-api` is the *browser*, client-side, via
`fetch()`, using the `API_BASE` value resolved once from the `?api_base=` query param (or
`localStorage`) as documented in [timeline.html:65059-65072](../../timeline.html#L65059-L65072).
That mechanism was verified working earlier this session (e2e suite re-run, all 3 tests green) and
is unaffected by which port the static file server happens to run on, because the backend's
presigned URLs are relative paths (verified at
[backend/timeline-storage/src/memory/object_store.rs:1-8](../../backend/timeline-storage/src/memory/object_store.rs#L1-L8)),
not host-baked absolute URLs.

So this plan interprets "ensure it's connecting to timeline-api" as: **ensure both processes are
each a single, fresh, healthy instance of the current code** — not as a new networking or
handshake mechanism. Getting the browser to point at the right backend origin (the `?api_base=`
step) stays a manual, one-time-per-browser step, already solved and documented in
[README.md:51-79](../../README.md#L51-L79). This is a scope decision, not a gap — see critique C3.

## Design

### One script, parameterized per role

`scripts/dev-up.sh {backend|static}` — a single script, not two, so the "kill whatever's on this
port, then take it over" logic isn't duplicated:

```
scripts/dev-up.sh backend   # clears port $PORT (default 3000), execs `cargo run -p timeline-api`
scripts/dev-up.sh static    # clears port $STATIC_PORT (default 8000), execs `python3 -m http.server $STATIC_PORT`
```

Behavior for either role:

1. Find any process currently listening on the target port (`lsof -ti tcp:$PORT`).
2. If found, **print its PID and full command line** (`ps -p $pid -o pid,cmd=`) before touching
   it — never kill silently. Send `SIGTERM`; wait up to 3s; `SIGKILL` if it's still alive. This
   makes "restart" mean "the old one is provably gone," closing the exact gap the user described.
3. `exec` into the real long-running process (`cargo run -p timeline-api` / `python3 -m http.server`)
   — not a background-and-detach. `exec` replaces the script's own process with it, so:
   - The script exits, but only as the same process now running the server — there's no separate
     wrapper/PID-file bookkeeping to fall out of sync with reality.
   - Whoever terminates that process (Ctrl+C in a terminal, or VS Code's task-terminate button)
     kills the real server directly, with nothing left behind.
4. No custom "wait until healthy" polling loop is added. Both `cargo run -p timeline-api` (prints
   `timeline-api (local dev, in-memory storage) listening on http://127.0.0.1:3000` — see
   [backend/timeline-api/src/main.rs:88](../../backend/timeline-api/src/main.rs#L88)) and
   `python3 -m http.server` (prints `Serving HTTP on 0.0.0.0 port 8000 ...`) already announce
   readiness on stdout. VS Code's own background-task mechanism (below) watches for that line
   instead of the script re-implementing a health check.

`scripts/dev-down.sh {backend|static|all}` — the same port-clearing step as a standalone command,
for manual terminal use and for this plan's own test script's teardown (see Testing below).

### VS Code tasks (the "button")

`.vscode/tasks.json` with three entries:

- **"Dev: Backend (timeline-api)"** — runs `scripts/dev-up.sh backend`, `isBackground: true`, a
  problem matcher whose `background.endsPattern` matches `listening on http://`.
- **"Dev: Static server (timeline.html)"** — runs `scripts/dev-up.sh static`, `isBackground: true`,
  `endsPattern` matching `Serving HTTP on`.
- **"Dev: Start local environment"** — a compound task, `dependsOn` the two above,
  `dependsOrder: parallel`, marked `"group": {"kind": "build", "isDefault": true}` so it's the
  target of VS Code's native "Run Build Task" action (Command Palette, the Terminal menu, or
  `Ctrl+Shift+B`/`Cmd+Shift+B`) — no extension required, and this works the same in VS Code Web.

Each server gets its own terminal pane (clearer output than interleaving both in one shared
script), and restarting is just "run the build task again": the new `dev-up.sh backend` invocation
kills whatever's currently on port 3000 — including the previous task's own still-running
`cargo run` — before taking over. The old terminal pane will show its process being killed; that's
expected, not an error, and is exactly the "no ambiguity about which version is running" property
being built here (see critique C1).

Stopping cleanly: VS Code's own per-terminal "kill" control (trash-can icon in the Terminal panel,
or the "Tasks: Terminate Task" command) sends the signal directly to the `exec`'d process — no
custom stop task needed.

### Testing

`scripts/test-dev-up.sh`, run manually (not part of `cargo test`/CI — it manages real ports and
long-running processes, which the Rust workspace's own test suite deliberately never does):

1. Occupy port 3000 with a throwaway process (`python3 -m http.server 3000 &`) to simulate a stale
   leftover.
2. Run `scripts/dev-up.sh backend &`; poll `curl -sf http://127.0.0.1:3000/conversations` until it
   answers (reusing the same wait pattern as [e2e/upload-flow.spec.js:22-35](../../e2e/upload-flow.spec.js#L22-L35)).
3. Assert exactly one process is now listening on 3000, and that it's a `cargo`/`timeline-api`
   process, not the throwaway one (`lsof -ti tcp:3000` plus a `ps` command-line check) — proves
   the stale process was actually replaced, not merely joined by a second one.
4. Re-run `scripts/dev-up.sh backend` a second time (idempotency check); assert the same
   single-process-on-3000 property still holds, and that the PID changed (proves the second run
   genuinely replaced the first, not a no-op).
5. Repeat 1-4 for the static server on port 8000.
6. Teardown via `scripts/dev-down.sh all` in a trap, regardless of pass/fail, so a failed test run
   never leaves stray servers behind.

This is the closest available substitute for unit tests on a script whose entire job is process
lifecycle management — CLAUDE.md's "write tests for all code additions" applies to this script
too, so it doesn't ship untested, but a `cargo test`-style in-process test doesn't fit a script
whose whole point is spawning and killing real OS processes on real ports.

## Self-critique log

### C1 [RESOLVED]: background/detached vs. foreground/`exec`'d process model
Original concern: a launcher script could either detach both servers into the background (tracked
via PID files) or run them in the foreground as the direct target of `exec`, with VS Code's task
terminal attached directly to each. Detached is more "fire and forget," but risks the exact problem
being fixed — an orphaned background process nobody remembers is still running, invisible once its
launching terminal/task closes.
**Resolution:** foreground `exec`, no PID files — see [§Design, "One script, parameterized per
role," step 3](#one-script-parameterized-per-role). VS Code's task UI becomes the single source of
truth for "is it running": a live task pane means it's running, closing/terminating the task means
it's dead. Applied in the design above.

### C2 [RESOLVED]: safety of killing "whatever is listening on the port"
Original concern: unconditionally killing any process bound to port 3000/8000 could kill something
unrelated to this project if the ports happen to collide with other work on the same machine.
**Resolution:** scoped to two project-specific, already-documented dev ports only (never a
system-wide port scan), and the script prints the victim's PID and full command line before
sending any signal — see [§Design, step 2](#one-script-parameterized-per-role). Given these are
personal dev-only ports already dedicated to this project per the existing README, the residual
risk is low and now auditable rather than silent.

### C3 [RESOLVED]: what "ensure the http server is connecting to timeline-api" actually requires
Original concern: the phrase implies some connection to verify between the two server processes,
but no such connection exists — the only consumer of `API_BASE` is the browser, client-side, via a
mechanism already built and verified working this session.
**Resolution:** scope explicitly narrowed to "both processes are a single fresh healthy instance
of current code" — see [§Scope clarification](#scope-clarification-what-ensure-the-http-server-is-connecting-to-timeline-api-means-here).
No new code addresses browser-side `API_BASE` wiring; that stays the existing, already-working
`?api_base=` query-param mechanism.

### C4 [RESOLVED]: how to test a process-lifecycle script under CLAUDE.md's test-everything rule
Original concern: CLAUDE.md requires tests for all code additions and 100% coverage, but that
framework (`cargo test`, `cargo-llvm-cov`) is built around the Rust workspace and doesn't apply to
a bash script managing real OS processes and ports.
**Resolution:** a dedicated bash integration test, `scripts/test-dev-up.sh`, run manually — see
[§Testing](#testing). It exercises the actual failure mode this plan exists to fix (a stale
process on the port) and the idempotency property (re-running converges, doesn't duplicate).

### C5 [RESOLVED]: does this need a companion `dev-down.sh`?
Original concern: the VS Code task flow doesn't strictly need a stop script (VS Code's own
task-terminate button kills the `exec`'d process directly), so a separate down script could be
scope creep.
**Resolution:** kept, but minimally — `scripts/dev-down.sh` reuses the same port-clearing step as
a standalone command. It's required by `scripts/test-dev-up.sh`'s teardown (§Testing, step 6) and
doubles as a manual escape hatch for plain-terminal users who aren't going through VS Code tasks at
all. Not wired into the VS Code task JSON itself.

## Open questions for review

- Is `scripts/` (bash) the right home/language, or would you rather this live under `backend/` or
  as a `justfile`/`Makefile` target? Bash was chosen only because the README already documents raw
  shell commands and the repo has no existing task-runner convention.
- Confirm the foreground/`exec` + VS Code-task-as-source-of-truth model (C1) matches what you
  pictured by "a button that reduces terminal usage" — versus, say, a status-bar toggle from an
  extension, which would be a materially different (and heavier) approach.
