# Experiment: which written instruction actually reduces unshared vocabulary

## Why this exists

Across this project's conversations, the user has repeatedly had to stop and ask what a word meant
— *fixture*, *e2e*, *sharing*, *provenance*, *V2a*, *C11*. Several written rules were drafted to
stop that, each rejected in turn for being a description of a virtue rather than something
applicable consistently, correctly, and rapidly.

Rather than argue about which wording would work, this measures it. Separate model instances are
given the same questions under different written instructions, and their answers are compared.

## The candidate instructions

Seven were considered in conversation. They are listed here in full so the three that were tested
can be located among them, and so the four that were not tested are on the record as untested.

| Label | Instruction | Tested? |
|---|---|---|
| **baseline** | No writing instruction at all. | **Yes** |
| **A** | "Write at the level of language you would expect from someone familiar with object-oriented software engineering texts and practice." The user's own criterion. | **Yes** |
| **B** | "Use a word only if it appears in the user's messages, in CLAUDE.md, or in the codebase." | No — withdrawn before testing. The set includes documents written by Claude, so Claude's own coinages (`V2a`, `C11`) certify themselves. The user identified this defect directly. |
| **C** | "Use a word only if it appears somewhere in this prompt, or if it is one of the file paths or code names listed." | **Yes** |
| **D** | Describe the thing first, put any short name in parentheses after, never the name alone. | No |
| **E** | One word, one meaning — from the aerospace documentation standard ASD-STE100. | No |
| **F** | A table of short forms already known to have cost the user a question, each with its required replacement. | No |
| **G** | Name a target reading level. | No — rejected by the user before testing: the user's own vocabulary is large, so reading level measures the wrong thing, and it cannot admit technical words that are genuinely shared. |

Only baseline, A and C were tested. B, D, E, F and G were not run.

## What each instance is given

Three files, identical across conditions except the instruction.

### The shared setup and questions (`scenario.txt`, given verbatim to every run)

```
You are helping on a small software project. Answer the three questions below.

The project: a web page lets a person send in a file of their saved chat history. A Rust server
takes that file, keeps it, and can send back a copy with marks added showing which messages look
angry or critical. The web page is one large HTML file. There are tests that drive a real browser
against the real running server.

Files you may refer to:
- timeline.html — the web page
- backend/timeline-api/src/routes/detect.rs — server code that adds the marks
- backend/timeline-api/src/processing.rs — server code that reads and stores a sent-in file
- backend/timeline-storage/src/memory/conversations.rs — keeps a list of chats in memory
- e2e/views.spec.js — the browser tests
- scripts/dev-up.sh — starts the server

Questions:

1. The browser tests all use one running server, so data left behind by one test can change what
   the next test sees. How should we stop that from happening?

2. Adding the marks takes about 20 seconds, because the server reads and re-reads the whole
   sent-in file once for each batch of chats it works through. How would you make that faster?

3. Two runs of the same test showed the chats in a different order each time. What could cause
   that?

Answer all three. Keep each answer to about 100 words.
```

The three questions were chosen because each is the shape of question that has previously caused
unshared vocabulary in this project:

1. **How do we stop shared state between tests** — previously produced *fixture*, *e2e*, *hook*.
2. **How would you make this faster** — previously produced *sharing*, a name invented for a
   mechanism that did not exist.
3. **Why did two runs differ** — previously produced *non-determinism*, *hash seed*,
   *iteration order*.

The setup text is deliberately written in plain words. It names no mechanism, so any term of art in
an answer came from the model, not from the prompt.

### The instruction text, appended per condition

**baseline** (`cond-baseline.txt`) — empty file, zero bytes.

**A** (`cond-A.txt`):
```
WRITING INSTRUCTION: Write at the level of language you would expect from someone familiar with
object-oriented software engineering texts and practice.
```

**C** (`cond-C.txt`):
```
WRITING INSTRUCTION: Use a word only if it appears somewhere in this prompt, or if it is one of the
file paths or code names listed above. For anything else, write what you mean instead of naming it.
```

### What each instance is told to do

Each is a separate model instance with no knowledge of this experiment or of the conversation that
produced it. Each is told to read its two files, follow any instruction in the second, write its
three answers to a numbered output file, and reply with the single word `done`. Writing to a file
rather than replying keeps the answers out of the coordinating conversation's context and makes the
scoring mechanical.

## Measurements

Two, per the user's request.

### 1. Rate of words not present in the prompt

For each answer: every word is matched against the set of words the instance was given — the setup
text, its own instruction text, and the boilerplate telling it where to write. File paths are split
on `/`, `_`, `.` and `-` so that a path counts as having supplied its parts.

- **Words** — total word count of the answer.
- **Outside prompt** — how many of those words, counted with repeats, do not appear in the given set.
- **Distinct** — how many *different* such words.
- **Rate** — outside prompt ÷ words.

**This measure has a known defect, stated before the results.** It cannot tell an ordinary English
word from a term of art. *Rather*, *whose* and *instead* count against a run exactly as
*hash-keyed* and *tie-break* do. So a low rate does not mean clear writing, and the numbers are
only interpretable alongside the word lists themselves.

### 2. Wall-clock time per run

Taken from the duration the harness reports for each instance, cross-checked against the write time
of the output file.

## Procedure

Ten runs per condition was the target. All runs of a batch are launched in a single message so they
execute concurrently and no condition is advantaged by running under lighter load.

## Stopping criterion

Stated plainly, because the first attempt did not have one and that was a mistake:

- **Planned**: 10 runs per condition, 30 total.
- **Actually applied to the first six runs**: none. Six runs were launched, all six finished, and
  the results were reported because two of them behaved so unlike the other four that continuing
  without explaining the difference would have wasted hours.
- **Applied from here**: stop and report if any single run exceeds five minutes, or if a defect in
  the instruction text under test is discovered, rather than completing the remaining runs against
  a stimulus already known to be flawed.

## Known defects in this design

1. **The setup text is written in plain words**, which likely pulls the baseline toward plain
   answers. The baseline is therefore a strong one, and the measured benefit of any instruction is
   probably smaller here than it would be against a technically-worded prompt.
2. **The scoring cannot distinguish jargon from ordinary English** (see above).
3. **Temperature is not controllable** through the harness used to launch instances, so variation
   between runs of one condition is narrower than the user asked for.
4. **Condition C's text, read literally, bans ordinary connective words** — *but*, *not*, *new*,
   *clear* — because they do not appear in the prompt. This was not intended and was discovered
   only after running it. It means condition C as tested is stricter than the rule it was meant to
   represent.
