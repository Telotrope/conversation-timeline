# Vocabulary pilot — results

Run of [docs/plans/2026-09-30-vocabulary-pilot-RUN-THIS.md](../plans/2026-09-30-vocabulary-pilot-RUN-THIS.md) on 2026-09-30.
Replies verbatim: [2026-09-30-vocabulary-pilot-raw-responses.md](2026-09-30-vocabulary-pilot-raw-responses.md).
Numbers: [2026-09-30-vocabulary-pilot-measurements.csv](2026-09-30-vocabulary-pilot-measurements.csv).

## Read this first

**There was one run per condition. With one run each, a difference between two conditions cannot be
told apart from a difference between two runs of the same condition.** This pilot shows whether the
machinery works and whether the answers differ in kind. It cannot rank the instructions. Nothing below
should be read as "condition X beat condition Y."

## Headline observations

0. **Three of the seven agents read the experiment's own instructions before answering.** From the
   agent transcripts: `vocab-baseline` ran `sed -n 1,125p docs/plans/2026-09-30-vocabulary-pilot-RUN-THIS.md`,
   and `vocab-G1` and `vocab-H` each read lines 1–80 of it. That file names the conditions, says the
   study is about vocabulary, and lists the words it is watching (*fixture*, *e2e*, *hook*,
   *provenance*). The other four (A, C, E, G2) listed `docs/plans`, so they saw the filename but, as
   far as the transcripts show, not its contents. The answers from baseline, G1 and H are therefore
   answers from agents that may have known they were being measured, and on what. This was not
   foreseen by the plan and nothing in the prompt prevented it. It is a reason, on its own, not to
   compare those three against the other four.
1. **This time the instructions reached the agents.** The check agent (`vocab-H`) reported
   its own heading present, the stale heading absent, and quoted its rule's first sentence word for word
   (I compared it to `~/.claude/agents/vocab-H.md`, line 85). Separately, and without being asked,
   the `vocab-baseline` agent reported that its instructions had no vocabulary section while the
   on-disk `CLAUDE.md` did. That is correct for the control, and it is a second sign that
   `omitClaudeMd` held back the session's copy. The check agent made one tool call even though it was
   told not to read any file. I did not look at what it read.
2. **All seven gave substantially the same answer.** Each found the `POST /_dev/reset` route and the
   `test.beforeEach` in [e2e/views.spec.js](../../e2e/views.spec.js), and each found that
   [e2e/upload-flow.spec.js](../../e2e/upload-flow.spec.js) never resets. Five (baseline, A, E, G1, G2)
   recommended Playwright's `test.extend` with `{ auto: true }`. `vocab-H` mentioned it as an
   alternative. `vocab-C` recommended copying `resetBackend()` into a shared module and never used the
   word *fixture*. Every tool call by every agent (4–6 each; transcripts checked) was a read-only shell
   command — `git log`, `git show`, `grep`, `sed -n`, `cat`, `ls`. None ran tests, started the
   server, or changed a file.
3. **The rough prose the plan feared from the lookup rule did not appear.** In the earlier attempt the
   lookup rule produced sentences like "the server keeps code which the browser tests can use". Here
   `vocab-C` and `vocab-E` read like the others: the same reading-ease band (77.6 and 79.1, against
   76.5–86.0 overall), and `vocab-C` still wrote "hook", "reset route" and "test runner".
4. **The vocabulary measures are small, close together, and move as much between two classifier
   runs as between conditions.** After blind classification, unshared naming words per answer ranged
   from 0 to 8. A second blind classification of the same items (see "Measurement problems") gave
   3 to 12, disagreed on 12 of 51 words, and changed every condition's count by 0 to 4 — G2 went from
   0 to 3. The differences between conditions are the same size as the difference between two
   classifier runs, so this column cannot separate the conditions even before the one-run problem.
5. **The words the plan was written about barely appear.** *provenance* and *idempotent*: none of the
   seven. *e2e* as a word: twice (`vocab-C` quoting a commit subject; `vocab-E`, "your e2e tests").
   *hook*: A once, C twice, G1 three times. *fixture*: every condition except baseline and C
   (A 4, E 3, G1 3, G2 4, H 1), and every one of those five defined it the first time it was used.
6. ***fixture* counts as shared vocabulary under this plan's definition.** `FIXTURE` is a constant in
   [e2e/views.spec.js:19](../../e2e/views.spec.js#L19),
   [e2e/upload-flow.spec.js:14](../../e2e/upload-flow.spec.js#L14) and
   [backend/timeline-core/tests/dedup_regression.rs:11](../../backend/timeline-core/tests/dedup_regression.rs#L11),
   and there is a `tests/fixtures/` folder. So the measure cannot score *fixture* against any run,
   and the stale rule's claim that *fixture* is "not in either" is wrong about the code.
   *idempotent* is shared for the same reason: it is part of the test name
   `dedup_is_idempotent_on_already_deduplicated_real_data`.

## What each column means

- **total_words**: words in the answer's prose after code blocks, inline code, file paths, link
  targets and commit hashes are removed.
- **content_words**: prose words tagged noun, proper noun, verb, adjective or adverb. Closed-class
  words (articles, pronouns, prepositions, conjunctions, auxiliaries) are excluded here.
- **unshared_mechanical**: nouns, proper nouns and adjectives not found in the shared set (defined
  below). This is before the blind classification, so it still includes everyday words like *clean*.
- **unshared_naming_words**: the subset of those that a blind classifier called a *name* (a term a
  reader would need to already know or be told), not everyday English. **rate** = that ÷
  content_words.
- **names_invented**: distinct terms used as a label for a thing or technique that are not in the
  shared set. Split into single words (from the column above) and multi-word phrases the classifier
  pulled from each sentence, such as "reset route" or "automatic fixture". A phrase counts as shared
  only if it appears word for word in the question or the common `CLAUDE.md` body, or as a
  snake_case, kebab-case or camelCase string in tracked code.
- **definitions**: sentences the blind classifier said explain what a term means.
- **parentheticals / em_dashes / gloss_markers**: raw counts of `( … )`, `—`, and *that is / in
  other words / i.e. / meaning*. The plan listed these as definition signs. Most parentheticals
  here were not definitions (e.g. "(I read this in the code)"), so they are reported but not
  interpreted.
- **flesch_reading_ease**: higher is easier. **flesch_kincaid_grade**: US school grade.
- **wall_clock_s, tokens, tool_uses**: as reported by the harness for each agent.

## Results

| condition | words | unshared naming (rate) | names invented (single / phrases) | definitions | reading ease | grade | seconds | tokens |
|---|---|---|---|---|---|---|---|---|
| vocab-baseline | 487 | 3 (0.012) | 23 (3 / 20) | 0 | 79.0 | 5.0 | 40.7 | 48,594 |
| vocab-A | 423 | 3 (0.015) | 17 (3 / 14) | 1 | 79.4 | 5.3 | 42.4 | 47,921 |
| vocab-C | 375 | 4 (0.022) | 12 (3 / 9) | 0 | 77.6 | 6.0 | 40.7 | 45,646 |
| vocab-E | 372 | 4 (0.022) | 18 (3 / 15) | 1 | 79.1 | 5.1 | 46.8 | 47,767 |
| vocab-G1 | 398 | 8 (0.039) | 23 (6 / 17) | 1 | 76.5 | 5.2 | 41.7 | 46,324 |
| vocab-G2 | 370 | 0 (0.000) | 12 (0 / 12) | 1 | 86.0 | 3.7 | 28.9 | 43,464 |
| vocab-H | 474 | 1 (0.004) | 18 (1 / 17) | 1 | 77.1 | 5.8 | 34.9 | 46,577 |

Unshared naming words by condition: baseline *baseline, fakes, uncommitted*; A *hook, runner, zig*;
C *hook, runner, uncommitted*; E *endpoint, runner, shutdown*; G1 *Gerard, hook, Meszaros,
relational, rollback, xUnit*; G2 none; H *idiomatic*. (The counts above count occurrences, so C's
4 is *hook* twice.)

Every one of the five definitions is of *fixture*.

## The expected directions

| Expectation from the plan | Held? |
|---|---|
| C and E show the fewest unshared naming words, and may read worse for it | **Did not hold.** C and E (4 each) are tied for second-most; only G1 has more. G2 (0) and H (1) have the fewest. C does have the highest grade level (6.0), but E is at 5.1, the same band as the others. |
| G1 shows the highest grade level and probably the most unshared words | **Split.** Most unshared naming words: held (8, citing Meszaros's *xUnit Test Patterns* and database transaction rollback). Highest grade level: did not hold. G1 is 5.2, below C (6.0), H (5.8) and A (5.3). |
| G2 sits lower on grade level than G1 | **Held** (3.7 against 5.2). G2 is also the easiest to read, the shortest, and the fastest. |
| A and baseline closest to each other | **The data cannot speak to this.** No distance measure was defined. On each column taken alone, the closest condition to baseline varies (grade: G1; unshared rate: A and H; word count: H). |
| H shows fewer invented names than baseline without the word-count cost of C and E | **Direction held, with one run each:** H 18 against baseline 23, at 474 words against 487. But C and E are not longer than the others. They are among the shortest, so there was no word-count cost to avoid. |

Two things weaken every row of that table beyond the one-run limit: baseline, G1 and H read the
experiment plan, which lists these very expectations, before answering; and the unshared-naming
counts shift by up to 4 words depending on which classifier run is used (next section).

## Measurement problems — read before using any number above

- **Everyday words scored against runs, as the plan warned.** Part-of-speech tagging alone left
  *clean, usual, half, expensive, crashes, repository* as unshared. The blind classification removed
  them. The unshared_naming_words column depends on that classifier's judgement, and the classifier is
  a model. Its labels, with the reason it gave for each, are in the appendix.
- **Two blind classifiers ran, and they disagree.** Two classifier agents were launched from this
  session on the same shuffled, unlabelled items; both wrote to the same output file, and the numbers
  above come from the one that wrote last (the one whose label scheme — name / everyday / fragment —
  the join step reads). I did not intend two runs: the second was launched during a stretch of this
  session I have no record of (see the last item). Comparing them anyway, on the 51 words still
  unshared after all fixes:

  | condition | unshared naming words, classifier 1 (used) | classifier 2 |
  |---|---|---|
  | vocab-baseline | 3 | 4 |
  | vocab-A | 3 | 6 |
  | vocab-C | 4 | 5 |
  | vocab-E | 4 | 7 |
  | vocab-G1 | 8 | 12 |
  | vocab-G2 | 0 | 3 |
  | vocab-H | 1 | 3 |

  All 12 disagreements are classifier 2 calling a word a term where classifier 1 called it everyday
  or a tokenizer fragment: *guard(s)*, *pins*, *collision(s)*, *repository*, *drives*, *apache-2*,
  *port-3000*. Classifier 2 also found 9 definitions against classifier 1's 5 — the extra four gloss
  "production build" (H), "browser-side state" (G1), and "Playwright" (A and E, "Playwright, the test
  runner …"). Under classifier 2, G2 no longer has the fewest unshared naming words (tied with H at 3,
  baseline 4), and C and E still do not have fewer than baseline.
- **The shared set was taken from the common `CLAUDE.md` body, not the file on disk.** The on-disk
  file has the uncommitted vocabulary section, which lists *fixture, provenance, idempotent* as words
  not to use. Using it made those words "shared". I used the `vocab-baseline` agent body (the
  `CLAUDE.md` text common to all seven conditions) instead. Each condition's own vocabulary section
  was not added to the shared set.
- **Identifiers came from code with comments and string literals stripped**, and from path
  components, across 122 tracked files. `.md`, `.txt`, `.csv`, `.snap` and `.lock` files were not
  scanned, because they are prose or data rather than identifiers.
- **Singular/plural matching was fixed partway.** The first combined pass scored *worker* and
  *transaction* as unshared, though `workers` and `transactions` are in the code. The numbers above
  are after the fix. The classifier's labels for those two words stay in the appendix but no longer
  count.
- **Most "names invented" are ordinary compound descriptions.** "spec files", "clean state", "file
  order", "safety checks": the phrase check is exact-match, so almost any two-word noun phrase not
  written in `CLAUDE.md` counts. This column mostly tracks how many noun phrases an answer has. I
  would not use it without a second, stricter pass.
- **Definitions were judged by the blind classifier, not by the plan's marker list.** The markers
  missed every actual definition. All five are "A fixture is …" or "'fixture' means …" sentences.
  Sentence-form definitions of other terms that the classifier did not flag, such as H's
  "(the one deployed to AWS Lambda)", are not counted.
- **Readability splits bullet items poorly.** Items without a full stop can merge into long
  "sentences". This affects all seven similarly, but not identically.
- **A bug of mine in the join step, now fixed.** The first version passed backticked labels to `git
  grep` through a shell, which ran the backticked words as commands. Nothing destructive ran: all were
  "not found", except `env`, which only printed the environment into a variable. That version's
  shared verdicts for backticked labels were wrong. The numbers above come from the fixed version,
  which calls `git` directly with no shell.
- **The classifier agent ran under this session's `CLAUDE.md`**, which includes the stale lookup rule.
  It saw no condition labels, so whatever bias that caused applies equally across conditions.
- **Part of this session is missing from my record.** Partway through, my turn ended without my
  having finished rewriting the measurement script. When I resumed, a rewritten script was in the
  scratchpad and had already been run, and the agent transcripts show a second blind classifier
  launched from this session in that interval. I infer both were mine, from the part of the session
  I cannot see; I have not confirmed it. I read the script in full and reran it myself before using
  its output. The scripts lived in the scratchpad and are not kept; the method is written out above so
  it can be rebuilt.

## Appendix — blind word classification

The classifier saw each word with one sentence of context, shuffled across all seven answers, with no
condition labels. Condition labels were joined back afterwards.

| word | class | classifier's reason | conditions |
|---|---|---|---|
| apache-2 | fragment | tokenizer fragment of license name | baseline, A |
| baseline | name | labels a specific experiment condition | baseline |
| clean | everyday | ordinary adjective | H, baseline, G2, C, G1 |
| collision | everyday | ordinary word for clash | E, A |
| collisions | everyday | ordinary word for clashes | C |
| crashes | everyday | ordinary word for failing abruptly | C, baseline |
| custom | everyday | ordinary adjective | baseline |
| drives | everyday | ordinary figurative sense of operating | G1 |
| endpoint | name | technical term for server route | E |
| expensive | everyday | ordinary sense: costly in time | H, G1 |
| fakes | name | testing term of art for stand-ins | baseline |
| gerard | name | person's name cited as source | G1 |
| guard | everyday | ordinary word for a protective check | G2 |
| guards | everyday | ordinary word for protective checks | A |
| half | everyday | ordinary fraction word | G2, baseline |
| hook | name | testing term of art | G1, C, A |
| idiomatic | name | programming jargon for conventional style | H |
| lack | everyday | ordinary verb meaning missing | baseline |
| leftover | everyday | ordinary word for remaining | C |
| leftovers | everyday | ordinary word for remains | baseline |
| meszaros | name | person's name cited as source | G1 |
| omission | everyday | ordinary word for leaving something out | G1 |
| pins | everyday | ordinary sense: fixes at a value | G2 |
| port-3000 | fragment | tokenizer fragment of port number | E |
| relational | name | database category term of art | G1 |
| repository | everyday | ordinary word for store/collection | G1, H |
| rollback | name | database technique term of art | G1 |
| runner | name | part of term of art test runner | E, C, A |
| shutdown | name | labels a specific code function | E |
| switch | everyday | ordinary noun meaning change-over | A |
| transaction | name | database term of art — *shared after plural fix; not counted* | G1, H |
| uncommitted | name | version-control term of art | C, baseline |
| usual | everyday | ordinary adjective | H, baseline, G1 |
| worker | name | test runner term for parallel process — *shared after plural fix; not counted* | A, H, baseline |
| xunit | name | book title / technical term | G1 |
| zig | name | name of a software tool | A |
