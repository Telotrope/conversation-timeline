#!/usr/bin/env python3
"""Report the words in a piece of writing that the user has never used.

WHAT COUNTS AS SHARED, AND WHY IT IS NARROW
-------------------------------------------
Shared vocabulary is what the *user* has written, and nothing else:

  * every message the user typed, across every conversation in this project
  * the text of the question being answered

Deliberately NOT shared, because the user has said so directly:

  * code Claude wrote.  `idempotent` was counted as shared by an earlier
    version of this check because it sits inside the test name
    `dedup_is_idempotent_on_already_deduplicated_real_data` -- an identifier
    Claude wrote and the user has probably never read.  A word buried in one
    snake_case identifier is not shared vocabulary with anybody.
  * plan and analysis documents Claude wrote.  Same argument: `V2a` and `C11`
    mean only what a document Claude authored says they mean.
  * `fixture` in particular.  An earlier check cleared it because `FIXTURE` is
    a constant in three test files.  The user had already said the word cost
    them a question.  When a measurement clears a word the reader has said is
    unclear, the measurement is wrong, not the word.

A spelling dictionary cannot do this job: /usr/share/dict/american-english
contains idempotent, fixture, provenance, coercion, hook, adapter and port.
Neither can a word-frequency list: it marks `port` common, because a port is a
harbour.

HOW ORDINARY ENGLISH IS EXCLUDED
--------------------------------
No part-of-speech tagger is installed, so "naming word" is approximated by
"not in the closed-class list below".  That list is written out in full so it
can be argued with.  The count alone is not the output -- the flagged words
are printed too, because a number nobody can check is what produced two void
experiments.
"""
WHAT THIS DOES NOT DO, MEASURED RATHER THAN GUESSED
---------------------------------------------------
1. A word the user typed *while objecting to it* counts as shared.  Quoted and
   backticked spans are stripped, which is why `hook` is correctly flagged --
   the user only ever quoted it from an error message.  But `fixture` and
   `idempotent` were typed bare in complaints ("Idempotent was ruled in as a
   word inside an identifier you wrote"), so they pass.  There is no mechanical
   way to tell "used" from "complained about" when the word is unquoted.
2. The ordinary-English list is too short, so roughly two thirds of what this
   flags is not a vocabulary problem.  On the pilot control answer it flagged
   `spec`, `in-memory`, `route`, `config`, `caller`, `fakes` -- which is the
   signal -- alongside `clean`, `possibly`, `absent`, `choice`, `contains`,
   `cover`, `crashes`, `depending`, `empties`, `feature`, `forget`, `fresh`,
   `half`, which is noise.  No part-of-speech tagger is installed and there is
   no pip, so the list is hand-written and will stay incomplete.
   The fix, not yet applied: exclude ordinary words with a word-frequency list
   *first*, then decide shared-ness from the user's corpus.  That is a
   different use of a frequency list from the one rejected earlier -- there it
   was used to certify `port` as ordinary, which is wrong because a port is a
   harbour; here it would only remove glue before the real test runs.
3. Same word, different meaning, is invisible.  The user has typed `port`
   eleven times, always meaning a TCP port, never the architecture term.  This
   marks it shared for both senses.

Because of 2, treat the rate column as indicative and read the flagged word
list, which is printed for exactly that reason.

import argparse
import glob
import json
import os
import re
import sys

TRANSCRIPTS = os.path.expanduser(
    "~/.claude/projects/-home-molinemc-workspace-conversation-timeline/*.jsonl"
)

# Closed-class words plus the commonest verbs, adjectives and adverbs. Being in
# this list means a word is never reported, however unshared it is.
ORDINARY = set("""
a an the this that these those there here it its itself
i you he she we they them their his her our your my me us him
is are was were be been being am s re ve ll d t don doesn didn isn aren wasn
weren won wouldn couldn shouldn hasn haven hadn can cannot could will would
shall should may might must do does did done doing have has had having get
gets got getting go goes went going come comes came make makes made making
take takes took taken give gives given put puts keep keeps kept let lets
say says said tell tells told ask asks asked see sees saw seen look looks
looked find finds found know knows knew think thinks thought want wants
wanted need needs needed use uses used using work works worked working
run runs ran running read reads write writes wrote written call calls called
show shows showed shown mean means meant leave leaves left start starts
started stop stops stopped add adds added change changes changed
of in on at to from by for with without about into over under between across
through during before after since until against among within upon per via
and or but nor so if then than as because while when where why how whether
though although unless however instead rather also too very much many more
most less least own same other others another each every all any both few
some such no not only just even still yet again once always never often
sometimes usually already ever almost quite well better best worse worst
what which who whom whose
one two three first second third next last new old good bad big small large
long short high low right wrong true false sure clear likely unlikely
possible probably certainly perhaps maybe
time times way ways thing things part parts point points case cases
lot lots kind sort bit end ends side sides top bottom
""".split())

WORD = re.compile(r"[A-Za-z][A-Za-z'-]*")
HARNESS_BLOCK = re.compile(r"<[^>]+>.*?</[^>]+>", re.S)
HARNESS_TAG = re.compile(r"<[^>]+>")
INTERRUPT = re.compile(r"\[Request interrupted[^\]]*\]")
# Spans the user quoted rather than wrote: double quotes, single quotes around
# a single word, and backticks.
QUOTED = re.compile(r"\"[^\"]{1,80}\"|`[^`]{1,80}`|'[A-Za-z][A-Za-z'-]{1,30}'")


def user_words():
    """Every word the user has typed, across every conversation in this project.

    User records store content as a list of blocks; only `text` blocks are
    typed by the user. `tool_result` blocks are command output and are not the
    user's words. Harness-injected wrappers are stripped.
    """
    words = set()
    blocks_seen = 0
    for path in glob.glob(TRANSCRIPTS):
        with open(path) as handle:
            for line in handle:
                try:
                    record = json.loads(line)
                except json.JSONDecodeError:
                    continue  # partial write at end of an active transcript
                if record.get("type") != "user":
                    continue
                # Conversation summaries are written by the harness and stored
                # under the user role. They are not the user's words, and they
                # paraphrase Claude's own prose back into the corpus.
                if record.get("isCompactSummary") or record.get("isMeta"):
                    continue
                content = record.get("message", {}).get("content")
                if isinstance(content, str):
                    blocks = [{"type": "text", "text": content}]
                elif isinstance(content, list):
                    blocks = content
                else:
                    continue
                for block in blocks:
                    if not isinstance(block, dict) or block.get("type") != "text":
                        continue
                    text = block.get("text", "")
                    text = HARNESS_BLOCK.sub("", text)
                    text = HARNESS_TAG.sub("", text)
                    text = INTERRUPT.sub("", text)
                    # A word the user only ever quoted is not a word they
                    # adopted. Every use of `fixture` and `idempotent` in this
                    # corpus is the user quoting the word back while objecting
                    # to it; counting those as shared would clear exactly the
                    # words they complained about. Bare uses survive, which is
                    # why `e2e` and `adapter` stay shared.
                    text = QUOTED.sub(" ", text)
                    if not text.strip():
                        continue
                    blocks_seen += 1
                    words.update(w.lower() for w in WORD.findall(text))
    return words, blocks_seen


def strip_code(text):
    """Remove fenced blocks, inline code, paths and link targets.

    A word inside `backend/timeline-api/src/routes/detect.rs` is a reference the
    reader can open, not vocabulary they have to know.
    """
    text = re.sub(r"```.*?```", " ", text, flags=re.S)
    text = re.sub(r"`[^`]*`", " ", text)
    text = re.sub(r"\]\([^)]*\)", "] ", text)
    text = re.sub(r"\S+/\S+", " ", text)
    return text


def check(text, shared):
    prose = strip_code(text)
    words = [w.lower() for w in WORD.findall(prose)]
    flagged = {}
    for word in words:
        if word in ORDINARY or word in shared:
            continue
        flagged[word] = flagged.get(word, 0) + 1
    return words, flagged


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="+", help="text files to check")
    parser.add_argument("--question", help="file holding the question, whose words count as shared")
    parser.add_argument("--quiet", action="store_true", help="counts only, no word lists")
    args = parser.parse_args()

    shared, blocks = user_words()
    if args.question:
        with open(args.question) as handle:
            shared = shared | {w.lower() for w in WORD.findall(handle.read())}
    print(f"shared vocabulary: {len(shared)} distinct words, from {blocks} user messages", file=sys.stderr)

    for path in args.files:
        with open(path) as handle:
            words, flagged = check(handle.read(), shared)
        total = sum(flagged.values())
        rate = 100 * total / len(words) if words else 0.0
        print(f"{os.path.basename(path)}\t{len(words)}\t{len(flagged)}\t{total}\t{rate:.2f}")
        if not args.quiet and flagged:
            for word, n in sorted(flagged.items(), key=lambda kv: (-kv[1], kv[0])):
                print(f"\t{n:>3}  {word}")


if __name__ == "__main__":
    main()
