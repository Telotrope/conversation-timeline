# Experiment v2: which written instruction reduces unshared vocabulary

Supersedes [2026-09-30-vocabulary-instruction-experiment.md](2026-09-30-vocabulary-instruction-experiment.md),
whose results are in [2026-09-30-vocabulary-instruction-results.md](../analysis/2026-09-30-vocabulary-instruction-results.md).
That first attempt had three flaws the user identified: it invented a task instead of using the real
project instructions, it invented questions instead of using the user's own, and its scoring could
not tell an ordinary English word from a term of art. All three are fixed here.

## What changes from v1

| v1 | v2 |
|---|---|
| A made-up writing instruction pasted into the prompt | The real `CLAUDE.md`, varied per condition — the same file that governs behaviour in ordinary use |
| An invented scenario written in deliberately plain words | The real repository, with no scenario at all |
| Three invented questions | Three of the user's own questions, edited only to stand alone |
| Three conditions | Seven |
| Scoring counted every word not in the prompt, including *but* and *whose* | Scoring counts only content words, exempting closed-class words by part of speech |
| One measurement plus wall clock | Seven measurements |
| Answers discarded after scoring | Answers kept verbatim in a separate file so later measurements can be run without re-running anything |

## Conditions

Seven. Each is the real `CLAUDE.md` with one section swapped; everything else in the file is
identical across conditions. The file is restored and its hash verified between conditions.

| Label | The vocabulary section reads |
|---|---|
| **baseline** | *(section absent — the file as it stood before any vocabulary rule was added, keeping the user's original "Plain language" section)* |
| **A** | "Write at the level of language you would expect from someone familiar with object-oriented software engineering texts and practice." |
| **C** | The lookup rule: use a word only if the user has used it, or it is a file path or code name in the repository — **now with the part-of-speech exemption below**. |
| **E** | One word, one meaning, after the aerospace documentation standard ASD-STE100 — **with the same exemption**. |
| **G1** | "Write at a postgraduate reading level — Flesch–Kincaid grade sixteen or above." The highest named band in the standard formulas. |
| **G2** | "Write for a New York Times reader." |
| **H** | "Before giving something a name, check whether that name appears in the user's own messages or in this repository. If it does not, describe what the thing does instead of naming it." |

**The part-of-speech exemption**, added verbatim to C and E:

> Ordinary English is always allowed and this rule never applies to it: articles, pronouns,
> prepositions, conjunctions, auxiliary verbs, and everyday verbs, adjectives and adverbs. The rule
> applies only to words that *name* something — a thing, a technique, a category, a mechanism.

This exists because v1's condition C, read literally, forbade *but*, *not*, *new* and *clear*, and
the instance obeying it wrote prose that has to be read twice.

**H is not a word filter.** C and E are checked against every word; H fires only at the moment of
naming something. That difference is the point of testing it: v1 measured the per-word check
costing four to five times the model's working time.

**baseline is included although the user did not list it.** Without a control, none of the six
differences mean anything.

## What each instance is asked

The user's own questions, edited only where they referred to earlier conversation. Each instance
gets one question, alone, with no framing, in the real repository — as the user would type it.

**All three are answerable by reading.** The first attempt used two questions that were not: one
asked what the test suite's duration is, so every instance ran the suite, and ten of them ran it at
once against one machine and one port; another asked about the cost of re-parsing, and two
instances wrote benchmark programs to measure it. Both are recorded in
[the results file](../analysis/2026-09-30-vocabulary-experiment-v2-results.md) and both are retired.
They produced contention the instances noticed and reported, a run lasting 1,156 seconds, and code
the user did not ask for.

1. "It seems like tests can't assume a neutral starting state and simply need to reset it
   explicitly. Is there a standard way to do this that you can employ?"
   *(Verbatim except capitalisation and a missing space.)*

2. "Why does the backend always detect? It's not clear to me whether that is different from the
   'Classify with AI' button available elsewhere in the interface, and it will certainly confuse
   users."
   *(Verbatim except that a back-reference to the preceding message is dropped.)*

3. "Where did work on this project leave off, and what is verified versus what is not?"
   *(Original: "Where did I leave work on the conversation timeline? What's verified and what's
   not?" — reworded to third person so it does not assume the reader is the user.)*

Each was chosen because it previously produced unshared vocabulary here: question 1 produced
*fixture*, *e2e* and *hook*; question 2 produced "the heuristic" and "AI" with no noun a user would
ever see; question 3 produced *V2a*, *C10* and *code-level only*.

**One constant line is appended to every question**, identically in every condition, so it cannot
favour one over another:

> Answer by reading only — do not run the tests, and do not write code.

This is task framing, which the user asked to avoid, and it is here against that preference for one
reason: without it, instances write throwaway programs and run the suite, which is both contention
and code the user has said they do not want.

## Runs

Ten per condition, seventy total. Conditions run one at a time, because every instance shares one
working directory and `CLAUDE.md` can only hold one variant at a time. Within a condition, all ten
run concurrently, so no condition is advantaged by lighter load.

Each instance writes its answer with a shell command rather than the editing tool. In v1 that tool
blocked for exactly 600 seconds, three times, on two runs — thirty wasted minutes each — while a
shell command wrote the same file in under three seconds.

## Measurements

All seven, computed over the saved answers. Every one can be recomputed later from the raw answers
file without re-running anything.

1. **Unshared naming words, and rate.** Content words in the answer that do not appear in the
   shared set — the question text, `CLAUDE.md`, and identifiers and paths in the repository.
   Closed-class words (articles, pronouns, prepositions, conjunctions, auxiliaries, and a list of
   everyday adverbs and verbs) are excluded before counting, so *but* and *whose* can no longer
   score against a run. Rate is unshared naming words ÷ content words.
2. **Wall clock.** Total elapsed time per run, with any blocked tool call reported separately so a
   harness fault can never be mistaken for an effect of the instruction.
3. **Tokens used.** As reported for each instance.
4. **Names invented.** Terms used as a label for a thing or technique that appear nowhere in the
   shared set. Candidates are found mechanically; classifying each as an invented name, an
   established term of art, or an ordinary word is a judgement made over a list with the condition
   labels removed.
5. **Total words in the answer.**
6. **Definitions included.** Parenthetical glosses, em-dash glosses, and the markers *that is*,
   *in other words*, *i.e.*, *meaning*. An approximation, and counted identically across conditions.
7. **Readability, by the formulas editors use.** Flesch Reading Ease and Flesch–Kincaid Grade Level,
   both computed from a syllable count. The syllable counter is a heuristic, so the absolute grade
   numbers are rough; the comparison between conditions is what matters.

## Files this experiment will produce

Three, all committed.

**`docs/analysis/2026-09-30-vocabulary-experiment-v2-raw-responses.md`** — every answer verbatim,
labelled by condition, question and run number, with nothing removed or summarised. This exists so
any measurement invented later can be run against the same answers without repeating the
experiment. It holds no analysis. *(Placed with the analysis rather than under `docs/reports/`
because the reports directory is for artefacts generated by the product for customers, per this
project's own filing rule; this is collected data.)*

**`docs/analysis/2026-09-30-vocabulary-experiment-v2-measurements.csv`** — one row per run, one
column per measurement, so the numbers can be re-sorted or re-plotted without parsing prose.

**`docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md`** — the analysis. Column meanings
stated before any table. Per-condition summaries with the spread across runs, not just averages.
Quoted passages showing what each condition actually reads like. Anything that failed, was lost, or
could not be measured, stated plainly. Any conclusion the data does not support, marked as such.

## Stopping criterion

- Stop and report when all ten runs of all seven conditions are done.
- Stop early and report if any single run exceeds five minutes, or if a defect is found in an
  instruction being tested, rather than finishing runs against a stimulus already known to be broken.
- Restore `CLAUDE.md` and verify its hash after every condition, and once more at the end. The file
  carries uncommitted work by the user; losing it is a worse outcome than losing the experiment.

## Known limits, stated before the results

1. **Temperature cannot be set** through the harness that launches instances, so the spread within a
   condition is narrower than the user asked for.
2. **The syllable counter behind the readability scores is a heuristic**, so grade levels are
   approximate.
3. **"Names invented" needs judgement.** It is made over a condition-blind list, but it is not
   mechanical, and a different judge could count differently.
4. **One question per instance** means each condition's ten runs are spread across three questions,
   so per-question sample sizes are three or four, not ten.
