# Vocabulary pilot — run this

**To start, the user says:** *"Run the vocabulary pilot in docs/plans/2026-09-30-vocabulary-pilot-RUN-THIS.md"*

This file is written for a session that has no memory of the conversation that produced it.
Everything needed is here. Read it fully before acting.

## What this measures

Whether a standing instruction in the project instructions changes the vocabulary a model uses when
answering a real question about this repository — specifically whether it stops using words the user
has to ask about (*fixture*, *e2e*, *hook*, *provenance*, and invented names).

Seven instruction variants, one question, one run each. **Seven agents total.**

## Two earlier attempts produced nothing. Do not repeat their mistakes.

Both are written up; read them if anything here seems arbitrary.

- `docs/analysis/2026-09-30-vocabulary-instruction-results.md`
- `docs/analysis/2026-09-30-vocabulary-experiment-v2-results.md`

The three mistakes, in order of cost:

1. **The instruction never reached the agents.** 46 runs, 3.87 million tokens, all executed under the
   same instruction. Editing `CLAUDE.md` mid-session does **not** change what a subagent is governed
   by — it receives a copy captured when the session started. Verified directly. **This is why Step 1
   exists. Do not skip it.**
2. **Extra instructions were appended to the question** — "answer by reading only", "write to this
   path", "reply with one word", "do not summarise". That put a variable inside the text being
   measured. **Nothing is appended to the question. Ask it and nothing else.**
3. **Scoring could not tell a term of art from ordinary English.** One instrument called *port*
   common; the next called *whose* unshared. Step 3 says what to count.

## Step 1 — Gate. Do this first and stop if it fails.

Spawn ONE agent with `subagent_type: "vocab-H"` and this exact prompt:

> Report on your own instructions only. Do not read any file. Answer these three, under 80 words total.
>
> 1. Does the heading "## Check names, not words" appear in your instructions? Answer YES or NO.
> 2. Does the heading "## Only use words that are already in the project" appear in your instructions? Answer YES or NO.
> 3. Quote the first sentence that follows whichever of those two headings you do have.

| Q1 own rule | Q2 stale rule | What it means |
|---|---|---|
| YES | NO | Working. Go to Step 2. |
| YES | YES | `omitClaudeMd` is not suppressing the session's cached instructions, so every condition would also carry the lookup rule and the comparison is void. **Stop. Report this.** |
| NO | either | The agent definitions are not being delivered. **Stop. Report this.** Do not fall back to putting the instruction in the prompt — that is the thing being avoided. |
| *"Agent type not found"* | — | `~/.claude/agents/` is not registered with this session. **Stop.** It needs a session that started after that directory existed. |

## Step 2 — The seven runs

The seven agent types already exist at `~/.claude/agents/vocab-*.md`. Each one's body is the entire
`CLAUDE.md` with one section swapped, and each sets `omitClaudeMd: true`.

| `subagent_type` | Condition |
|---|---|
| `vocab-baseline` | control — no vocabulary section at all |
| `vocab-A` | "Write at the level of language you would expect from someone familiar with object-oriented software engineering texts and practice." |
| `vocab-C` | Use a word only if the user has used it, or it is a file path or code name in this repository — with a part-of-speech exemption for ordinary English |
| `vocab-E` | One word, one meaning — same exemption |
| `vocab-G1` | "Write at a postgraduate reading level — Flesch–Kincaid grade sixteen or above." |
| `vocab-G2` | "Write for a New York Times reader." |
| `vocab-H` | Check the name at the moment of naming, rather than checking every word |

Spawn all seven **in one message so they run concurrently**, each with this prompt and **nothing else
added to it**:

> It seems like tests can't assume a neutral starting state and simply need to reset it explicitly. Is there a standard way to do this that you can employ?

This is the user's own question, verbatim. It is answerable by reading; on 46 prior runs no agent
needed to run anything. If an agent does run something, that is data — report it, do not suppress it.

**Each agent's reply is the answer.** Capture it from the reply. Do not tell the agent to write a
file; that is appended framing. Save all seven verbatim to
`docs/analysis/2026-09-30-vocabulary-pilot-raw-responses.md`, labelled by condition, nothing removed.

## Step 3 — Measurements

Per answer, into `docs/analysis/2026-09-30-vocabulary-pilot-measurements.csv`:

1. **Total words.**
2. **Unshared naming words + rate.** Content words absent from the shared set — the question text,
   `CLAUDE.md`, and identifiers and paths in the repository. **Exclude closed-class words first**
   (articles, pronouns, prepositions, conjunctions, auxiliaries, everyday adverbs and verbs) so
   *but* and *whose* cannot score against a run. Rate = unshared naming words ÷ content words.
3. **Names invented.** Terms used as a label for a thing or technique that appear nowhere in the
   shared set. Find candidates mechanically; classify them over a list with the condition labels
   removed.
4. **Definitions included.** Parenthetical glosses, em-dash glosses, and the markers *that is*,
   *in other words*, *i.e.*, *meaning*.
5. **Readability.** Flesch Reading Ease and Flesch–Kincaid Grade Level.
6. **Wall clock**, from each agent's reported duration.
7. **Tokens used**, as reported.

## Step 4 — Report

Write `docs/analysis/2026-09-30-vocabulary-pilot-results.md`. State column meanings before any table.

**Say this plainly and do not bury it: with one run per condition, a difference between two
conditions and a difference between two samples of the same condition are indistinguishable.** This
pilot can show whether the machinery works and whether answers differ qualitatively. It cannot rank
the instructions. Any sentence implying otherwise is wrong.

Expected directions, so "did they vary as expected" is answerable rather than vibes:

- `vocab-C` and `vocab-E` should show the fewest unshared naming words, and may read worse for it —
  in an earlier attempt the lookup rule produced "the server keeps code which the browser tests can
  use, code which stops … from keeping all chats and all marks" instead of "add a reset route".
- `vocab-G1` should show the highest grade level and probably the most unshared words.
- `vocab-G2` should sit lower on grade level than `vocab-G1`.
- `vocab-A` and `vocab-baseline` should be closest to each other.
- `vocab-H` should show fewer invented names than baseline without the word-count cost of C and E.

Report which of these held, which did not, and which the data cannot speak to.

## Step 5 — Clean up

`rm ~/.claude/agents/vocab-*.md`, then confirm they are gone. They are throwaway definitions and
should not outlive the pilot.

Check `git status --short` at the end. `CLAUDE.md` must be unchanged — the user has an uncommitted
edit in it and this experiment must not touch that file at all. Its hash before this work was
`2b50e5d794ee067e98d193c9ad80578c76f6aae928f7f60cce54d27a0369cf48`.
