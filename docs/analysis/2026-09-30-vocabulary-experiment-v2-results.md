# Experiment v2: void, and why

Design: [docs/plans/2026-09-30-vocabulary-instruction-experiment-v2.md](../plans/2026-09-30-vocabulary-instruction-experiment-v2.md).
Answers: [docs/analysis/2026-09-30-vocabulary-experiment-v2-raw-responses.md](2026-09-30-vocabulary-experiment-v2-raw-responses.md).

## Result

**Forty-six runs were collected across five labelled conditions. All forty-six ran under the same
project instructions. The comparison measured nothing.**

The design varied `CLAUDE.md` on disk between conditions, on the understanding — stated in the plan
and checked beforehand with a probe — that a separate model instance reads the project instructions
when it starts. The probe confirmed instances *have* the instructions. It did not check whether they
read them **from disk at that moment**, and they do not.

## The evidence

Two instances in the fifth condition reported, unprompted, that the instructions they had been given
did not match the file on disk. One gave hashes. Both were right.

A direct probe then settled it. The file on disk was replaced with the **baseline** variant, which
contains no vocabulary section at all. A fresh instance was asked to quote any section of its
instructions about word choice, and separately to run `md5sum` on the file:

| | |
|---|---|
| File on disk | `ef14656384605fff6d03107bf94067e6` — the baseline variant, **no vocabulary section** |
| `md5sum` run by the instance | `ef14656384605fff6d03107bf94067e6` — agrees, so it was looking at the right file |
| Section quoted from its own instructions | `## Only use words that are already in the project` — a section that exists **only** in the original file |

The instance was reading one thing and governed by another.

**Mechanism:** project instructions are captured when the coordinating session starts and handed to
each instance from that copy. Editing the file afterwards changes what an instance can *read*; it
does not change what governs it. Every one of the 46 runs was therefore governed by the file as it
stood at session start — the version carrying the "Only use words that are already in the project"
rule.

## What the labels mean now

Nothing, as a comparison. `baseline`, `A`, `C`, `E` and `G1` in the filenames record which variant
sat on disk while each run executed. Since that is not what reached the instance, differences
between the groups can only be sampling variation between instances given identical instructions.

Counting them anyway would produce a table that looks like a result and is not one. No measurement
table appears in this file for that reason.

## What is still worth keeping

- **46 answers to three of the user's real questions**, all under one known instruction, saved
  verbatim. Useful as a sample of what that instruction produces, and as material for any later
  measurement, but with no control to compare against.
- **The timings** in `durations.csv`: 46 runs, 146–352 seconds each once the read-only constraint was
  added. Also single-condition, so also not a comparison.
- **A confirmed fact about the harness** that any future attempt has to design around.

## Two earlier problems this attempt did fix

Both are worth keeping for the next attempt.

1. **Questions that force work produce contention.** The first two questions tried — what the test
   suite's duration is, and what re-parsing costs — made every instance run the suite or write a
   benchmark program. Ten instances did that at once against one machine and one port. They noticed
   and said so: *"other sessions appear to be answering the same question concurrently against the
   same machine and the same port-3000 server, which makes all of these timings mutually
   contaminated to an unmeasured degree."* One reported two test failures it could not reproduce
   afterwards. One run took 1,156 seconds.
2. **Adding "answer by reading only — do not run the tests, and do not write code" fixed it.** Run
   times went from a 104–1,156 second spread to 146–352 seconds, no instance wrote code, and
   `git status` showed no repository changes afterwards.

## What a third attempt would have to do

The plan's premise — vary the real `CLAUDE.md`, because that is what governs behaviour in ordinary
use — cannot be met from inside one session. Three ways round it, none free:

1. **Put the instruction in the prompt instead.** Works today, and is what the first attempt did.
   The user rejected it for good reason: it is not the real mechanism, and a prompt line may carry
   different weight than a standing project rule.
2. **One session per condition**, with the file set before each session starts. Faithful, and it
   needs the user to restart the coordinating session seven times.
3. **Test whether an instance running in its own checked-out copy reads that copy's instructions.**
   Unknown; one probe would settle it. If it does, conditions could be committed to separate
   branches and run without restarting anything.

Option 3 is the only one that is both faithful and automatable, and it is one cheap probe away from
being ruled in or out.

## Housekeeping

`CLAUDE.md` was restored from its backup after every condition and again at the end, each time
verified against `sha256 2b50e5d794ee067e98d193c9ad80578c76f6aae928f7f60cce54d27a0369cf48`. The
user's uncommitted edit to that file is intact. No repository files were changed by any run.
