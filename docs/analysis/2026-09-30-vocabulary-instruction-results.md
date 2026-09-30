# Results so far: which written instruction reduces unshared vocabulary

Design, prompts and candidate instructions are in
[docs/plans/2026-09-30-vocabulary-instruction-experiment.md](../plans/2026-09-30-vocabulary-instruction-experiment.md).
This file reports only what was observed.

## What ran and what did not

| Attempt | Conditions | Runs | Outcome |
|---|---|---|---|
| First | baseline, A, C | 20 launched in background | **All 20 lost.** The coordinating session ended while they were still working. Zero output files were written. Nothing was recovered. |
| Second | baseline, A, C | 6 launched in foreground, 2 per condition | All 6 completed and wrote their answers. |

The target is 10 per condition. **2 per condition were completed.** Everything below rests on
n=2, which is not enough to separate conditions on any measure, and is reported because two of the
six behaved so unlike the other four that continuing without explaining it would have wasted hours.

- **baseline** — no writing instruction.
- **A** — "Write at the level of language you would expect from someone familiar with
  object-oriented software engineering texts and practice." The user's own criterion.
- **C** — "Use a word only if it appears somewhere in this prompt, or if it is one of the file
  paths or code names listed above."

## Measurement 1: words not present in the prompt

**Column meanings.** *Words* — total words in the answer. *Outside prompt* — how many of those
words, counted with repeats, do not appear anywhere in the text that instance was given (the setup,
its instruction, and the boilerplate about where to write). *Distinct* — how many different such
words. *Rate* — outside prompt divided by words.

| Run | Condition | Words | Outside prompt | Distinct | Rate |
|---|---|---|---|---|---|
| baseline-01 | no instruction | 307 | 116 | 96 | 37.8% |
| baseline-02 | no instruction | 359 | 135 | 108 | 37.6% |
| A-01 | user's criterion | 296 | 140 | 121 | **47.3%** |
| A-02 | user's criterion | 308 | 159 | 135 | **51.6%** |
| C-01 | lookup rule | 299 | 32 | 22 | **10.7%** |
| C-02 | lookup rule | 350 | 10 | 8 | **2.9%** |

Condition C reduces the rate by roughly a factor of four to thirteen against baseline. Condition A
*raises* it by about a quarter to a third.

## Why that table does not mean what it appears to mean

The measure counts every word not in the prompt, and cannot tell an ordinary English word from a
term of art. Here are the actual words it flagged.

**A-01, flagged words that are ordinary English:** *after, an, as, before, between, call, clean,
come, cost, create, every, gain, give, hand, hoping, instead, into, items, likely, most, never, no,
only, own, rather, run, several, ship, slow, still, than, then, times, today, too, where, whose,
work.*

**A-01, flagged words that are genuinely terms of art:** *hash-keyed, hook, before-each, in-memory,
tie-break, timestamp, identifier, collection, release build, development builds, parsed, walk
order, unspecified.*

So of 121 distinct flagged words in the clearest answer produced by any condition, roughly a dozen
are words a reader might have to stop at. The rest are the ordinary furniture of English sentences.

**C-02's entire flagged list, all eight words:** *batches, changes, drives, let, reading, sends,
smaller, uses.* Not one of them is a term of art. The measure records a near-perfect score for an
answer that avoided technical words by not saying technical things.

**This is the same error made twice.** An earlier attempt at scoring used a ten-thousand-word
English frequency list, which the user rejected because it marked *port* and *adapter* as common —
they are, as a harbour and a plug. This measure fails in the mirror image: it marks *whose* and
*rather* as unshared. Neither instrument can see the distinction that matters.

## What the answers actually read like

All three were asked the same question: how to stop one browser test's leftover data from affecting
the next.

**Condition A** (the user's criterion):

> "Add a route the server only compiles into development builds, say `POST /_dev/reset`, that
> empties the in-memory list in `backend/timeline-storage/src/memory/conversations.rs`, and call it
> from a before-each hook in `e2e/views.spec.js`. Add a test that the release build has no such
> route, so the reset path can never ship."

**baseline** (no instruction):

> "Add a route the tests can call — something like POST /_dev/reset — that tells
> `backend/timeline-storage/src/memory/conversations.rs` to drop everything it holds, and call it
> from a per-test setup step in `e2e/views.spec.js`. Keep that route out of the build that ships,
> and add a test that fails if it ever appears there."

**Condition C** (the lookup rule):

> "the server keeps code which the browser tests can use, code which stops
> `backend/timeline-storage/src/memory/conversations.rs` from keeping all chats and all marks"

Condition C could not write *reset*, *route* or *endpoint*, because none appear in the prompt. It
produced a description that has to be read twice and still does not name the thing. **The condition
that scored best on the measure produced the worst writing**, and by a wide margin.

## The defect this exposed in the rule

The instance running condition C reported that the instruction also forbade **but, not, way, new,
clear** — ordinary connectives absent from the prompt.

The rule as written into `CLAUDE.md` has the identical flaw: *"Use a word only if it appears in the
user's own messages, in this file, or in the codebase"* forbids *but* on a literal reading. The
worked examples underneath make the intent clear, but the instruction sentence does not, and an
instance following it literally wrote prose no one would want to read.

## Measurement 2: time per run, and why it is not usable

All six were launched in one message and ran concurrently.

| Run | Reported duration | Output file written at | Implied start | Tool calls | Tokens |
|---|---|---|---|---|---|
| baseline-01 | 42.6 s | 01:41:16.238 | 01:40:33.6 | 4 | 40,415 |
| baseline-02 | 34.3 s | 01:41:16.322 | 01:40:42.0 | 4 | 39,737 |
| A-01 | 42.3 s | 01:41:26.810 | 01:40:44.5 | 4 | 40,176 |
| A-02 | 37.4 s | 01:41:19.900 | 01:40:42.5 | 4 | 39,925 |
| C-01 | **1,945.9 s** (32 m 26 s) | 02:13:09.611 | 01:40:43.7 | **7** | 48,763 |
| C-02 | **2,010.3 s** (33 m 30 s) | 02:14:21.333 | 01:40:51.0 | **7** | 54,390 |

*Implied start* is the file write time minus the reported duration. True starts are a few seconds
earlier, since each instance also replies after writing.

### Checking for mis-measurement

**Was the end of the slow runs recorded late?** No. Implied starts for all six cluster between
01:40:33 and 01:40:51 — an 18-second spread, which is what six instances launched together should
look like. If the two long durations were an artefact of late recording, their implied starts would
be pushed far earlier than the others. They are not. The files really were written 32 and 33
minutes after the other four.

**Did the slow runs get fewer resources, or start late in a queue?** The implied start times rule
out a late start. Contention is ruled out by the other four: all six ran concurrently under
identical load and four finished in under 43 seconds.

**Did the instruction make the model deliberate longer?** The token counts say no, and this is the
decisive evidence. The two slow runs used 48,763 and 54,390 tokens against roughly 40,000 for the
others — about 22% to 36% more. A run that spent fifty times the wall clock *thinking* would have
produced far more than a quarter more tokens. Fifty times the time for a quarter more output means
the instance was **blocked, not working**.

### The leading explanation

Both slow instances reported the same thing: the tool they were told to write with failed **three
times each**, with `PreToolUse hook did not respond before its timeout (host client may be
unreachable)`, and both fell back to a shell command to create the file. The tool-call counts
corroborate it exactly — 7 calls for each slow run against 4 for each fast one, a difference of
precisely three.

The arithmetic fits a fixed timeout:

- C-01: (1,945.9 s − ~40 s of normal work) ÷ 3 blocked calls ≈ **635 s per block**
- C-02: (2,010.3 s − ~40 s of normal work) ÷ 3 blocked calls ≈ **657 s per block**

Both land near ten and a half minutes, which is what a fixed timeout hit three times would look
like. The same hook failure occurred twice in the coordinating session itself, on a different tool,
so the fault is recurrent and environmental.

**Conclusion: there is no usable timing comparison between the conditions.** The difference is
explained by an environment fault that happened to strike two runs, not by the instruction under
test. Reporting a fifty-fold slowdown as an effect of condition C would be wrong.

**What is not explained:** why only the C runs hit the outage. With n=2 this cannot be settled. One
possibility consistent with the timestamps is that the outage began just after 01:41:26, when the
last fast run wrote successfully, and that the C instances — having more to work out under a
restrictive rule — reached their first write a few seconds later and fell inside the window. That
is a hypothesis fitted to six data points, not a finding.

## Findings

1. **Condition C reduces unshared words and makes the writing worse.** It wins its own measure by
   avoiding the words needed to say the thing. Observed on 2 of 2 runs.
2. **Condition A produced the clearest answers of the three** while scoring worst on the measure,
   because a term of art used correctly is clearer than a description that avoids it.
3. **The measure is not fit for purpose.** It cannot separate *hash-keyed* from *whose*. Both
   scoring instruments tried so far have failed in opposite directions.
4. **The rule in `CLAUDE.md` has a real defect**, found by running it: read literally it forbids
   ordinary connectives, and an instance obeying it literally wrote prose that needs rereading.
5. **No timing conclusion is available.** See above.
6. **The baseline is stronger than expected** — it named `POST /_dev/reset` and described a map
   with unfixed walk order without reaching for the word *HashMap*. Probably because the setup text
   is written in plain words, which pulls every condition toward plain answers, including the one
   with no instruction at all.

## What would need fixing before continuing

- Condition C's wording must exempt ordinary English and restrict itself to technical terms and
  names, or the remaining runs test a strawman.
- The measure needs to separate terms of art from ordinary words. The most promising option not yet
  tried: have a separate instance, shown the answers with condition labels removed, list the words
  a reader would have to stop at — using judgment for recognition, and a mechanical check against
  the prompt for certification.
- The setup text should probably be rewritten in ordinary technical prose, so the baseline is not
  artificially plain.
