# Fix the command-line test login script (deployment check D3)

## Context

[scripts/aws-dev-token.sh](../../scripts/aws-dev-token.sh) prints an access token for a user of the
deployed `dev` stack, so the API can be checked with `curl` from a terminal without the browser
(migration plan [E7 (line 1236)](2026-09-09-rust-aws-backend-migration.md#L1236)). It is the
command-line part of deployment check D3.

Run by the user on 2026-10-02, it failed before contacting Cognito:
`Error parsing parameter 'cli-input-json': Invalid JSON received`. The diagnosis is in
[the deployment-checks analysis](../analysis/2026-10-02-deployment-checks-status.md): aws-cli 2.37.8
appears to open its `--cli-input-json file://...` input twice. Reopening an ordinary file starts
again from the beginning; reopening `/dev/stdin` when it is a pipe returns the same, already-emptied
pipe. (Inferred from a named-pipe test that hung on the second opening; not traced.)

**Why the script avoids the obvious form.** The obvious command is
`aws cognito-idp initiate-auth --auth-parameters USERNAME=...,PASSWORD=...`. That is not an `aws`
weakness but a Linux one: every program's command line is readable by every user on the machine
(`ps`, `/proc/<pid>/cmdline`) while it runs. Any command that takes a password as an argument has
this problem, so the script passes the password some other way.

**Why the bug went unnoticed.** The test's stand-in `aws`
([test-deploy-scripts.sh:27-32](../../scripts/test-deploy-scripts.sh#L27-L32)) reads standard input
directly, once, and never opens the file named by `--cli-input-json`.

## Options

**A. Fix the script (recommended; the user asked to "fix and rerun").** Write the request to a
temporary file only the user can read, hand `aws` that file, delete it when the script exits.

**B. Remove the script.** Get a token by copying it out of the signed-in page instead (the browser's
developer tools, where the sign-in library keeps it), and turn off password sign-in on the `dev`
login client, which would close the migration plan's
[C27 (line 2096)](2026-09-09-rust-aws-backend-migration.md#L2096). Costs: copying a token by hand
every hour; D3's script part is dropped rather than passed. Listed because the user asked why a
script is needed at all; not recommended only because A is small and keeps `curl` checks a
one-liner.

## §1 The change (option A)

In [aws-dev-token.sh](../../scripts/aws-dev-token.sh):

1. `request="$(mktemp)"` (created readable and writable by the owner only, mode 600) and
   `trap 'rm -f "$request"' EXIT`, set **before** the password is read, so the file is removed on
   success, on an `aws` error, and on Ctrl-C. Not removed if the script is killed outright
   (`kill -9`) or the machine loses power; stated in the script's comment.
2. The existing Python one-liner writes the JSON to `$request` instead of a pipe; the password still
   reaches Python through its environment, never a command line.
3. `aws cognito-idp initiate-auth --cli-input-json "file://$request" ...`.
4. Update the header comment: why a file and not a pipe (this plan), how long the password sits on
   disk (the length of one `aws` call).

*Reuse check:* the existing Python one-liner still builds the JSON. `mktemp`
plus `trap` is the pattern [test-deploy-scripts.sh:17-18](../../scripts/test-deploy-scripts.sh#L17-L18)
already uses. Nothing new is added beyond the temporary file.

*Error paths:* a failed `aws` call exits non-zero with AWS's message on standard error (unchanged,
`set -euo pipefail`); the trap removes the file either way.

## §2 Tests

In [test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh). **This modifies a committed test,
which needs the user's approval** (approving this plan grants it):

1. The stand-in `aws`, for `cognito-idp`, reads the file named by `--cli-input-json` **twice**, the
   way the real one appears to, and fails with "Invalid JSON received" if the second read is empty.
   It records the file's path, its permissions, and its contents. It no longer reads standard input.
   (The current check at [line 90](../../scripts/test-deploy-scripts.sh#L90) reads the recorded
   standard input, so it changes to read the recorded file contents; its assertions stay the same.)
2. New: the request file had permissions 600 while `aws` read it.
3. New: the request file no longer exists after the script exits, both when `aws` succeeds and when
   the stand-in answers with an error (a second run with a failing stand-in).
4. Kept unchanged: prints the token; the password never appears on a command line.

Regression check: run the new test against the **current** script first; it must fail on item 1,
proving the stand-in now reproduces the real failure.

## §3 Rerun (deployment check D3)

The user runs, in a terminal:

```
TOKEN=$(scripts/aws-dev-token.sh <email>)
curl -i -H "Authorization: Bearer $TOKEN" https://84h93noqxd.execute-api.us-east-1.amazonaws.com/conversations
```

Pass: a token is printed and `curl` answers 200 with the user's conversations. Claude records the
result in the analysis. A failure is recorded as a failure, with its message.

## Self-critique log

### C1 [RESOLVED]: the password now touches the disk
Original concern: option A writes the password to a file, which a pipe avoided. **Resolution:** the
file is owner-only and deleted on exit ([§1 (line 44)](#L44)); the remaining risk (a leftover file
after `kill -9`) is stated in the script's comment. Option B avoids passwords on this machine
entirely and is offered ([Options (line 32)](#L32)).

### C2 [RESOLVED]: the test stand-in mimics a guess about `aws`
Original concern: "opens its input twice" is inferred, not traced; a stand-in built on it could
encode a wrong model. **Resolution:** the stand-in's double read is a stricter condition than the
real `aws` needs, so an ordinary file passes both, and the real check is §3's rerun against real
`aws` ([§3 (line 79)](#L79)).

### C3 [OPEN]: other `aws` versions
Only aws-cli 2.37.8 was tried. An ordinary file worked with it and is the input form AWS documents,
so other versions are expected to accept it. **Open:** revisit if the script fails after an `aws`
upgrade.
