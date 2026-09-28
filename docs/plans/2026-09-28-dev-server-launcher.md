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

### One script, parameterized per role — restart only when the running instance is actually stale

`scripts/dev-up.sh {backend|static}`. **Revised per user feedback: unconditionally killing
whatever's on the port is wrong.** `timeline-api` holds all state — every uploaded conversation and
confirmed flag — in memory only (`backend/README.md`'s persistence note; `run_locally` in
[backend/timeline-api/src/main.rs](../../backend/timeline-api/src/main.rs) wires nothing but
`InMemory*` stores). Killing a still-current backend for no reason doesn't just cost a few seconds
of relaunch time, it **silently deletes whatever the user was looking at.** The script must tell
"needs restarting" apart from "already correct, leave it alone."

The two roles need genuinely different staleness logic, because only one of them has anything that
can go stale in the first place:

**`backend`** — compiled code, so a running instance can be serving code older than what's on disk:

1. Run `cargo build -p timeline-api` unconditionally. This is the freshness check itself, not
   overhead to avoid — cargo's own incremental build already tracks the full dependency graph
   correctly (every crate, `Cargo.lock`, build scripts), which a hand-rolled "any `.rs` file newer
   than X" mtime scan in bash would get wrong in edge cases (a `Cargo.toml` dependency bump touches
   no `.rs` file, for instance). When nothing changed this finishes in well under a second — the
   real, disruptive cost this plan is fixing is the kill-and-relaunch-and-lose-all-state cycle, not
   this build check.
2. Hash the resulting binary (`sha256sum target/debug/timeline-api`).
3. Compare against `.dev-state/timeline-api.hash`, a marker written the last time this script
   actually started an instance (**not** a liveness-tracking PID file — see critique C6 for why
   that distinction matters).
4. Look up whatever's currently listening on `$PORT` (`lsof -ti tcp:$PORT`).
   - **Nothing listening:** start fresh regardless of the hash (nothing to preserve). Write the new
     hash to the marker, `exec cargo run -p timeline-api` (cheap — the binary's already built).
   - **Something listening, hash matches, and its command line looks like our binary**
     (`ps -p $pid -o cmd=` contains `timeline-api`): **do nothing.** Print `timeline-api already
     up to date (pid $pid) — leaving it running.` and exit 0 immediately. No signal is ever sent to
     that process.
   - **Something listening, but the hash differs (code changed) or the command line doesn't match
     our binary (something else is squatting on the port):** print the occupant's PID and full
     command line, `SIGTERM` it, wait up to 3s, `SIGKILL` if still alive — then start fresh as
     above. This is the original "provably gone, then replaced" behavior from the first draft of
     this plan, now used only when a restart is actually warranted.

**`static`** — `python3 -m http.server` re-reads `timeline.html` from disk on every request; it
never caches file content in the process, so **there is no such thing as a stale static-server
process** — restarting it can never make it serve fresher content than leaving it running would.
Its check is liveness-only, not freshness:
   - Something listening and it answers `curl -sf http://127.0.0.1:$STATIC_PORT/timeline.html`:
     leave it alone, exit 0.
   - Nothing listening, or the occupant doesn't answer correctly (wrong process on the port):
     kill-if-present (same print-PID-then-SIGTERM/SIGKILL as above) and start fresh.

Either way, once the script decides to actually start a process, it `exec`s into it (`cargo run
-p timeline-api` / `python3 -m http.server $STATIC_PORT`) rather than backgrounding-and-detaching —
the reasoning from the original draft (C1) still holds for *that* part: whichever terminal/task
ends up attached to it can terminate it directly, with nothing left behind. What's changed is only
*whether* that kill-and-start step happens at all on a given invocation.

`scripts/dev-down.sh {backend|static|all}` — unconditional port-clearing as a standalone command
(no freshness check — "down" always means down), for manual terminal use and for this plan's own
test script's teardown (see Testing below).

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
script), and re-running the build task is now the everyday way to "check whether I need a
restart" — most of the time (no code changed) it does nothing, per the revised design above.
**This changes what a task pane means, compared to the original draft:** a live pane is still
*sufficient* evidence the server is running, but is no longer *necessary* — the process that's
actually serving requests may be attached to an older pane from several task-runs ago, with more
recent runs each having printed "already up to date" and exited. Liveness truth stays with the OS
(`lsof` on the port), same as before; only the VS Code pane's role changes, from "the" source of
truth to "a" source of truth. `"presentation": { "reveal": "silent" }` on the two leaf tasks keeps
a no-op "already up to date" run from stealing focus or opening a new pane the user has to
dismiss, so re-running the build task stays cheap to do often (see critique C6).

On the runs that *do* find stale code, the old terminal pane shows its process being killed — that
part of the original design (provably-gone-then-replaced) is unchanged, just now conditional.

Stopping cleanly: VS Code's own per-terminal "kill" control (trash-can icon in the Terminal panel,
or the "Tasks: Terminate Task" command) sends the signal directly to the `exec`'d process — no
custom stop task needed. Note this only stops whichever instance that particular pane is attached
to; if you want the backend down entirely regardless of which pane (if any) is showing it, use
`scripts/dev-down.sh backend` from a terminal.

### Testing

`scripts/test-dev-up.sh`, run manually (not part of `cargo test`/CI — it manages real ports and
long-running processes, which the Rust workspace's own test suite deliberately never does):

1. **Stale-code case:** occupy port 3000 with a throwaway process (`python3 -m http.server 3000 &`)
   to simulate a leftover that isn't even the right program. Run `scripts/dev-up.sh backend &`;
   poll `curl -sf http://127.0.0.1:3000/conversations` until it answers (reusing the same wait
   pattern as [e2e/upload-flow.spec.js:22-35](../../e2e/upload-flow.spec.js#L22-L35)). Assert
   exactly one process is now listening on 3000 and it's the real `timeline-api`, not the
   throwaway one (`lsof -ti tcp:3000` plus a `ps` command-line check) — proves a wrong/stale
   occupant actually gets replaced.
2. **Already-current case (the behavior this plan revision exists to add):** with the real backend
   now running from step 1, note its PID, then re-run `scripts/dev-up.sh backend` with no source
   changes in between. Assert: (a) the PID on port 3000 is **unchanged**, (b) no `SIGTERM`/`SIGKILL`
   was sent (assert via a wrapper that fails the test if `kill` is invoked at all during this run —
   e.g. run under a stubbed `kill` shell function that records calls), (c) exit code 0. This is the
   test that would have caught the original design's data-loss bug: it fails loudly if a rerun ever
   restarts a server that didn't need it.
3. **Genuinely-changed case:** touch a source file under `backend/timeline-core/src/` (a trivial
   whitespace change is enough to force a rebuild), note the running PID, re-run
   `scripts/dev-up.sh backend`. Assert the PID **changes** and the new process answers
   `/conversations` correctly — proves a real code change is still detected and does trigger a
   restart, not just that restarts have become permanently disabled.
4. Repeat the three cases above for the static server on port 8000, adjusted for its liveness-only
   (never freshness-based) logic: stale/wrong occupant gets replaced; an already-running, correctly
   answering instance is left alone (its PID never changes across reruns, since staleness doesn't
   apply to it at all — there's no "genuinely changed" case to test here, per the design above).
5. Teardown via `scripts/dev-down.sh all` in a trap, regardless of pass/fail, so a failed test run
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
**Resolution:** foreground `exec` when a process is actually started, no PID files used for
liveness — see [§Design, "One script, parameterized per role"](#one-script-parameterized-per-role-restart-only-when-the-running-instance-is-actually-stale).
**Superseded in part by C6**: liveness (`lsof` on the port) is still ground truth and a live task
pane is still *sufficient* evidence of a running server, but after the C6 revision it is no longer
*necessary* — most reruns now find the server already current and exit without ever attaching a
pane to it. See C6 for the full revision and the VS Code-tasks section's "what a task pane means"
note.

### C6 [RESOLVED]: unconditionally killing the current process on every run is wrong
Raised by the user after reviewing the first draft: `timeline-api` holds all uploads and confirmed
flags in memory only (no database, no disk persistence — `backend/README.md`'s persistence note).
The original design's "always kill whatever's on the port, then start fresh" meant *every* rerun of
the launcher — including ones triggered just to check whether anything needed restarting — silently
discarded the user's in-progress session, and paid a multi-second rebuild+reboot cost, even when the
code hadn't changed at all.
**Resolution:** restart only when the running instance is provably stale, per role — the backend
compares a hash of the freshly-built binary against a marker recorded when it was last actually
(re)started, and only kills+restarts on a mismatch (or a wrong/absent occupant); the static file
server has no staleness concept at all (`python3 -m http.server` reads `timeline.html` from disk on
every request, so an old process can never serve stale content) and is left alone whenever it's
already up and answering correctly. Full logic in
[§Design, "One script, parameterized per role"](#one-script-parameterized-per-role-restart-only-when-the-running-instance-is-actually-stale);
the "already current, do nothing" and "genuinely stale, do restart" cases are both covered as
explicit test cases in [§Testing](#testing).

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
- Confirm the foreground/`exec`-when-actually-starting model, now combined with the C6
  skip-if-current logic, matches what you pictured by "a button that reduces terminal usage" —
  versus, say, a status-bar toggle from an extension, which would be a materially different (and
  heavier) approach.
- `.dev-state/timeline-api.hash` is a new small piece of on-disk state (git-ignored) this revision
  introduces to remember what was last started, purely so a rerun can tell "unchanged" from
  "changed" without re-deriving it from cargo's own build metadata by hand. Confirm you're fine
  with that file existing, versus, say, deriving the same fact some other way (e.g. always trusting
  whatever `cargo run` itself would decide to rebuild) — no other approach identified so far avoids
  needing *some* record of "what was running" to compare against.
