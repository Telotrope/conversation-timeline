#!/usr/bin/env python3
"""Deletes outdated copies from the Rust build folder, keeping everything the
current builds use (docs/plans/2026-10-02-automatic-build-cleanup.md).

Cargo keeps every outdated copy of every program and library it has built;
on 2026-10-02 backend/target reached 30 GB. This asks Cargo which files its
current builds use (its JSON build report), then, holding Cargo's own lock on
the build folder, deletes the rest of target/debug -- but never anything made
after this run started, so a build running alongside is never touched.

    scripts/clean-build.py                         # clean now
    scripts/clean-build.py --if-larger-than 12G    # only if target/debug is bigger
    scripts/clean-build.py --dry-run               # report only

Run automatically after every Claude Code turn by the Stop hook in
.claude/settings.json. Each run appends to <target>/clean-build.log.

Exit status: 0 cleaned, nothing to do, or another cleanup already running;
1 a Cargo command failed (nothing deleted); 3 Cargo's lock stayed held past
--lock-wait (nothing deleted).
"""

import argparse
import collections
import fcntl
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# What this repository builds: tests, programs and examples, and the server on
# its own as the browser tests and dev-up.sh build it (its dependencies'
# features differ from the whole-workspace build).
DEFAULT_LISTINGS = [
    "test --workspace --no-run",
    "build --workspace --bins --examples",
    "build -p timeline-api",
]

# Cargo names a build's files and folders <name>-<16 hex digits>.
BUILD_ID = re.compile(r"-([0-9a-f]{16})(?:\.|$)")

# The folders under target/debug holding one entry per build, named by BUILD_ID.
PER_BUILD_DIRS = ("deps", "examples", ".fingerprint", "build")

# Where Cargo keeps the hashed copies of programs: deps/ for binaries and
# tests, examples/ for examples (2026-10-02: measure_processing was missed).
PROGRAM_DIRS = ("deps", "examples")

# Cargo's locks on target/debug. Cargo 1.98 has three; holding them all keeps
# every build of this folder waiting (scripts/test-clean-build.sh checks that
# Cargo waits on each).
CARGO_LOCKS = (".cargo-lock", ".cargo-artifact-lock", ".cargo-build-lock")

SIZE_UNITS = {"": 1, "K": 1 << 10, "M": 1 << 20, "G": 1 << 30, "T": 1 << 40}


def parse_size(text):
    match = re.fullmatch(r"(\d+)([KMGT]?)", text.strip().upper())
    if not match:
        raise argparse.ArgumentTypeError(f"not a size like 12G: {text!r}")
    return int(match.group(1)) * SIZE_UNITS[match.group(2)]


def tree_size(path, vanished=None):
    """Bytes under `path`. A file a build deletes while we walk is not
    counted; its path is appended to `vanished` for the caller to report."""
    total = 0
    for root, dirs, files in os.walk(path):
        for name in files + dirs:
            entry = os.path.join(root, name)
            try:
                total += os.lstat(entry).st_size
            except FileNotFoundError:
                if vanished is not None:
                    vanished.append(entry)
    return total


class Log:
    def __init__(self, path):
        self.path = path

    def __call__(self, message):
        line = f"{time.strftime('%Y-%m-%dT%H:%M:%S')} {message}"
        print(line, flush=True)
        os.makedirs(os.path.dirname(self.path), exist_ok=True)
        with open(self.path, "a", encoding="utf-8") as f:
            f.write(line + "\n")


def current_builds(workspace, debug, listings, log):
    """Runs each listing with Cargo's JSON report. Returns (build ids, count of
    current builds per crate name), or raises RuntimeError naming the command
    and Cargo's error."""
    ids, units = set(), collections.Counter()
    for listing in listings:
        unhashed = set()
        command = ["cargo", *shlex.split(listing), "--message-format=json"]
        result = subprocess.run(command, cwd=workspace, capture_output=True, text=True)
        if result.returncode != 0:
            raise RuntimeError(f"`{' '.join(command)}` failed: {result.stderr.strip()[-2000:]}")
        for line in result.stdout.splitlines():
            try:
                message = json.loads(line)
            except json.JSONDecodeError as e:
                raise RuntimeError(f"`{' '.join(command)}` printed a line that isn't "
                                   f"Cargo's JSON ({e}): {line[:200]!r}") from e
            paths = []
            if message.get("reason") == "compiler-artifact":
                paths = list(message.get("filenames") or [])
                if message.get("executable"):
                    paths.append(message["executable"])
                units[message["target"]["name"].replace("-", "_")] += 1
            elif message.get("reason") == "build-script-executed":
                paths = [message["out_dir"]]
            for path in paths:
                found = [i for part in path.split("/") for i in BUILD_ID.findall(part)]
                if found:
                    ids.update(found)
                else:
                    unhashed.add(path)
        # Right after this listing, before the next one replaces the unhashed
        # name with its own build of the same program.
        add_program_ids(debug, ids, unhashed, log)
    return ids, units


def remove(path):
    if os.path.isdir(path) and not os.path.islink(path):
        shutil.rmtree(path)
    else:
        os.remove(path)


def add_program_ids(debug, ids, unhashed, log):
    """Programs Cargo reports by their unhashed name (target/debug/timeline-api)
    are the same file (same inode) as a hashed copy in deps/ or examples/. Adds that copy's
    build id to `ids`, which also keeps the program's .fingerprint entry:
    without it Cargo relinks the program (2026-10-02, 10 programs).

    The unhashed name holds only the copy built last, and this workspace
    builds timeline-api twice (whole workspace, and alone, with different
    features), so this runs after each listing, not once at the end."""
    wanted = set()
    for path in unhashed:
        try:
            wanted.add(os.stat(path).st_ino)
        except FileNotFoundError:
            log(f"Cargo reported {path}, but it doesn't exist; nothing to keep for it")
    for folder in PROGRAM_DIRS:
        base = os.path.join(debug, folder)
        for name in os.listdir(base) if os.path.isdir(base) else []:
            match = BUILD_ID.findall(name)
            if match and os.lstat(os.path.join(base, name)).st_ino in wanted:
                ids.add(match[-1])


def outdated(debug, ids, units, started):
    """Entries under `debug` that no current build uses and that are older
    than `started`."""
    found = []
    for folder in PER_BUILD_DIRS:
        base = os.path.join(debug, folder)
        if not os.path.isdir(base):
            continue
        for name in os.listdir(base):
            path = os.path.join(base, name)
            match = BUILD_ID.findall(name)
            if not match or match[-1] in ids:
                continue
            if os.lstat(path).st_mtime >= started:
                continue
            found.append(path)
    # Incremental folders (<crate>-<code>) can't be matched to a build id:
    # keep each crate's newest, as many as it has current builds.
    base = os.path.join(debug, "incremental")
    if os.path.isdir(base):
        by_crate = collections.defaultdict(list)
        for name in os.listdir(base):
            path = os.path.join(base, name)
            by_crate[name.rsplit("-", 1)[0]].append((os.lstat(path).st_mtime, path))
        for crate, entries in by_crate.items():
            entries.sort(reverse=True)
            found += [p for mtime, p in entries[units.get(crate, 0):] if mtime < started]
    return found


def acquire(paths, wait, poll=0.5):
    """Takes an exclusive flock on every one of `paths`, waiting up to `wait`
    seconds. All or none: when one is held elsewhere, the ones already taken
    are released before retrying, so this never holds one lock while waiting
    for another (no deadlock with a build taking them in its own order).
    Returns the open files, or None if they never all came free."""
    deadline = time.monotonic() + wait
    while True:
        handles = []
        for path in paths:
            handle = open(path, "a+")
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                handle.close()
                break
            handles.append(handle)
        if len(handles) == len(paths):
            return handles
        for handle in handles:
            handle.close()
        if time.monotonic() >= deadline:
            return None
        time.sleep(poll)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--workspace", default=os.path.join(REPO, "backend"),
                        help="the Cargo workspace (default: backend/)")
    parser.add_argument("--listing", action="append",
                        help="a Cargo command, without `cargo`, whose builds are current "
                             "(repeatable; default: this repository's three)")
    parser.add_argument("--if-larger-than", type=parse_size, metavar="SIZE",
                        help="do nothing unless target/debug (what this cleans) is bigger "
                             "than SIZE, e.g. 12G")
    parser.add_argument("--lock-wait", type=float, default=300,
                        help="seconds to wait for Cargo's lock (default 300)")
    parser.add_argument("--dry-run", action="store_true", help="report, delete nothing")
    args = parser.parse_args(argv)

    # cargo is installed outside the default PATH on this machine (see dev-up.sh).
    os.environ["PATH"] = os.path.expanduser("~/.cargo/bin") + os.pathsep + os.environ["PATH"]
    target = os.path.join(args.workspace, "target")
    debug = os.path.join(target, "debug")
    log = Log(os.path.join(target, "clean-build.log"))

    # Measured on target/debug, the part this cleans: the coverage and Lambda
    # builds beside it (5.7 GB on 2026-10-02) would otherwise keep the whole
    # folder over the limit and start a cleanup after every turn (plan C5).
    if args.if_larger_than is not None:
        size = tree_size(debug) if os.path.isdir(debug) else 0
        if size <= args.if_larger_than:
            return 0  # the common case after every turn: not worth a log line
        log(f"target/debug is {size / 1e9:.1f} GB, over {args.if_larger_than / 1e9:.1f} GB: cleaning")

    # Two sessions ending their turns together must not both clean.
    os.makedirs(target, exist_ok=True)
    own = acquire([os.path.join(target, ".clean-build.lock")], wait=0)
    if own is None:
        log("another cleanup is running; leaving it to finish")
        return 0

    started = time.time()
    try:
        ids, units = current_builds(args.workspace, debug, args.listing or DEFAULT_LISTINGS, log)
    except RuntimeError as e:
        log(f"nothing deleted: {e}")
        return 1

    # Cargo's own locks on the build folder: no build runs while we delete.
    cargo_locks = acquire([os.path.join(debug, name) for name in CARGO_LOCKS], wait=args.lock_wait)
    if cargo_locks is None:
        log(f"nothing deleted: Cargo's locks on {debug} stayed held for {args.lock_wait:.0f} s")
        return 3
    try:
        doomed = outdated(debug, ids, units, started)
        vanished = []
        size = sum(tree_size(p, vanished) if os.path.isdir(p) else os.lstat(p).st_size
                   for p in doomed)
        if vanished:
            log(f"{len(vanished)} files disappeared while being measured, e.g. {vanished[0]}")
        if args.dry_run:
            log(f"dry run: would delete {len(doomed)} entries, {size / 1e9:.2f} GB")
            for path in sorted(doomed):
                print(f"  {os.path.relpath(path, debug)}")
            return 0
        for path in doomed:
            remove(path)
        log(f"deleted {len(doomed)} outdated entries, {size / 1e9:.2f} GB; "
            f"{len(ids)} current builds kept")
        return 0
    finally:
        for handle in cargo_locks:
            handle.close()


if __name__ == "__main__":
    sys.exit(main())
