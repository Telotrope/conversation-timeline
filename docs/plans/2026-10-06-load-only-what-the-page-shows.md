# Load only what the page shows

**Status:** proposed 2026-10-06, revised twice the same day with the user's answers; not approved.
Open questions are in §11.

## 1. The problem

To open the timeline, the server rebuilds the user's whole export as one file and the page downloads
it: 63.6 MB for the user's `real-flags.json`. Every scan request re-reads and re-parses that same
file to reach five conversations. The user's question (2026-10-06) was why the page doesn't load
rows from the database instead. It can't today, because messages are stored only inside the
uploaded file. The migration plan decided not to store sessions because they could be "recomputed
from message timestamps already in DynamoDB"
([migration plan, line 81](2026-09-09-rust-aws-backend-migration.md#L81)). Message times never went
into DynamoDB, so the page recomputes everything from the downloaded file.

## 2. What the export holds, and what is kept

Measured on `real-flags.json` (61.8 MB):

| Part | Size | Kept? |
|---|---|---|
| Message text, yours (2,255 messages; largest 5.7 KB) | 0.41 MB | **yes, as rows** |
| Message text, Claude's (2,227; largest 18 KB) | 5.78 MB | **yes, as rows** |
| Tool results (web searches 32.4 MB, fetched pages, command output) | 39.9 MB | no (the user, 2026-10-06) |
| Tool calls (searches, commands, file edits) | 5.4 MB | no, except the files they made (§4) |
| Claude's thinking | 0.7 MB | no |
| Your attachments (8, with their text extracted, up to 32 KB) | < 0.1 MB | **yes, as files** (§4) |
| Files Claude wrote whose text is in the export (24 created files, 16 widgets) | ~1.5 MB | **yes, as files** (§4) |
| Your uploaded files (90 references) and files Claude made by running scripts (118: Word, PNG, PowerPoint…) | names only; their contents are not in the export | names only |

## 3. Where everything is kept

| Data | Store | Shape |
|---|---|---|
| Conversation records | DynamoDB `Conversations`, `CONV#…` (exists) | unchanged, plus a version number (§7) |
| **Sessions** | `Conversations`, new `SESS#{conversation}#{n}` rows | conversation, start, end, message counts, flag counts (§6) |
| **Messages** | `Conversations`, new `MSG#{conversation}#{message}` rows | conversation, session, sender, time, text, the names of files it mentions (§4), and, for yours, **its flags**: the automatic ones and your own as separate attributes |
| **Files** | S3, new `files/{user}/{conversation}/{message}/{name}` | the files of §4 |
| The `MessageFlags` table | **removed**: flags move onto the message rows | (its data was cleared on 2026-10-06) |
| The uploaded file | S3 `raw/…` | **deleted once processed** (the user) |

**Flags on the message rows** (the user, 2026-10-06). The migration plan kept the automatic and your
own flags apart so that the scan could never overwrite yours (§4.1). That stays true with both on
one row, as separate attributes. In the code, the scan writes through `AutoFlagWriter` and flag
saves through `UserFlagWriter`, as now, and neither can name the other's attributes. On AWS, each
function's permission names the attributes it may change (DynamoDB lets a policy restrict which
attributes an update touches). One query then reads every message with its flags.

Sessions are cut by the existing rule, a pause of 15 minutes or more, using the server's existing
port of it, `build_blocks` ([sessions.rs](../../backend/timeline-core/src/sessions.rs)), which no
route uses today. Which local day a session is drawn on stays in the browser, which knows the
viewer's time zone (migration plan §4.3).

**How big can the database get?** A DynamoDB table has no size limit; the limits are 400 KB per row
(the longest message here is 18 KB) and how fast one key can be written (C8). Storage costs about
$0.25 per GB a month (from memory, not checked), so 6 MB of rows per user is about $0.0015 a month.

## 4. Files Claude made, and your attachments

The page can only keep what the export contains:

- **Files Claude wrote with its file tool** (`create_file`): the export holds their full text, and
  later edits as find-and-replace steps (`str_replace`). Processing replays the edits in order to
  get each file's final text. One limitation: a file Claude changed afterwards by running a command
  can't be replayed, so the kept copy may be older than the final one. When a later command names
  the file, the copy is marked "may have been changed later".
- **Interactive widgets** (`visualize:show_widget`): their HTML is in the export; each is kept as a
  file.
- **Your attachments:** their extracted text is kept as a file.
- **Everything else** (your uploaded PDFs and images, and the Word documents, PNGs and PowerPoints
  Claude made by running scripts): the export holds only the name. The message row keeps the name,
  and Review shows it as "not included in the export".

In Review, a message that made or presented files lists them under its text, each a link
(`GET /files/{conversation}/{message}/{name}` answers with a short-lived download address). How the
browser should open a file that is a web page or SVG is Q3.

## 5. What the page loads

| When | Request | Returns | Size here |
|---|---|---|---|
| Opening the timeline | `GET /conversations` (exists) | conversation records | 73 KB |
| | `GET /sessions` (new) | every session, with its flag counts | ~70 KB |
| Right after, in the background | `GET /messages` (new) | your messages with their flags and flag handles | ~0.8 MB |
| With "Show Claude's replies" on | `GET /messages?replies` (new) | Claude's messages' text and file names | 5.8 MB, loaded once |
| A file's link in Review | `GET /files/…` (new) | a download address for it | |
| "Download annotated…" | `GET /export` (exists, rebuilt) | conversations from the rows: text, flags and file names | ~6.5 MB |

The Calendar and the Conversations tab are drawn from conversations and sessions alone. A session
that crosses midnight shows the same flags on each day it touches (the user's decision); today the
Calendar shows each day only its own messages' flags. Review and Analytics need your messages; if
opened before they finish loading, they say "Loading your messages…". Each request is one query,
paged through past DynamoDB's 1 MB per call (which also fixes the silent cut-off of `list_for_user`
noted in the screen-flow analysis).

**The annotated download changes.** It no longer reproduces the export, since tool calls, tool
results and thinking are not kept. It holds every conversation's messages as text, with your flags
and the names of their files. It can still be uploaded again: messages of text alone are a valid
export, and uploading it brings back your flags.

## 6. Session flag counts

The page's show switches combine flags three ways (automatic only, yours only, both with yours
winning), so each session row stores, for each of the three flags (critical, angry, ALL-CAPS), the
count under each combination: nine numbers. They change whenever a flag does:

- **A flag save** recounts that message's session in the same request: it reads the session's
  message rows (a session has at most 112 messages here) and rewrites the session's counts.
- **The scan** recounts each session it finishes.

## 7. Processing an upload

After the file is parsed (once, as now), processing:

1. cuts each new or extended conversation into sessions with `build_blocks`;
2. extracts the files of §4 and stores them;
3. writes the rows of the new messages, then each new or changed session's row;
4. writes the conversation's record last, with its new version;
5. deletes the uploaded file. A failed attempt leaves it, so a retry still has it.

On AWS each file of a batch is processed at the same time as the others. If two hold the same
conversation, both would read its record, add messages and write it back, the second write silently
replacing the first: a race that exists in the code built on 2026-10-05 too (C1). Processing
therefore writes the record only if its version is still the one it read (a DynamoDB conditional
write), and on a conflict re-reads and redoes that conversation. Session and message rows are keyed
by conversation and position, so a redo overwrites rather than duplicates.

The added-messages pieces built on 2026-10-05 (`additions/…`) and `conversation_rebuild.rs` are
replaced.

## 8. The scan

`POST /detect` reads your message rows instead of the file, and works for up to **10 seconds**
before answering with how many conversations it has done of how many in total (the user's
suggestion). The page asks again until all are done, so its bar shows a real percentage. Ten seconds
keeps each request well inside the deployment's 30-second limit
([template.yaml:148](../../infra/template.yaml#L148)).

## 9. Expected times

**Measured** on this machine on 2026-10-06, optimized build, in-memory stores, `real-flags.json`:
upload and processing 1.1 s; rebuilding the timeline download 1.0 s; downloading it on the same
machine 0.13 s; parsing it (in Node) 0.30 s; each of the 24 scan requests 0.6–0.7 s; the conversation
list 2 ms.

**Every other figure is an estimate.** Assumptions: the browser reaches the server at 100 Mbit/s
(the real speed through VS Code's forwarding isn't known); AWS runs these functions with 512 MB of
memory, which Lambda pairs with roughly 0.3 of a processor (from AWS's documentation as I remember
it, not checked), so work taking 1 s here takes about 3.5 s there; DynamoDB and S3 calls take
10–30 ms each.

| | Today, local | Today, AWS | After, local | After, AWS |
|---|---|---|---|---|
| **Opening the timeline** | 6.4 s | 9 s | 0.1 s | 0.5 s |
| **…and your messages, in the background** | (included above) | (included above) | 0.2 s | 1 s |
| **The scan** | 15 s (measured) | 50 s | under 1 s | 3 s |
| **Processing an upload** | 1.1 s (measured) | 5 s | 1.5 s | 10 s |
| **Showing Claude's replies** | already loaded | already loaded | 0.5 s once | 1.5 s once |
| **The annotated download** | 6.4 s | 9 s | 0.6 s | 2 s |
| **Saving one flag** | under 0.1 s | 0.2 s | under 0.1 s | 0.3 s |

"Today, local" opening and download times are the measured parts plus 5.1 s to send 63.6 MB at the
assumed speed. Processing on AWS gets slower: it writes about 4,500 message rows and 300 session
rows (25 per DynamoDB batch) and about 50 files, and may be slower still if DynamoDB limits the rate
of writes to one user's rows (C8).

**Cost per upload of this export** (AWS's published us-east-1 prices, from memory, not checked):
DynamoDB writes of about 8,000 write units, roughly $0.005–0.01; about 50 S3 writes, negligible.

## 10. Order of work

1. **Storage.** Session and message rows with flags on them (core types and ports, in-memory and
   DynamoDB adapters), the files store, paged queries, the conversation record's version; the
   `MessageFlags` table removed from the template.
2. **Processing.** Sessions, messages and files written with the versioned write and its retry; the
   files of §4 extracted; the upload deleted when done; the additions and `conversation_rebuild.rs`
   replaced.
3. **Flags and their counts.** Flag saves and the scan writing message rows; session counts kept
   current.
4. **Routes.** `GET /sessions`, `GET /messages`, `GET /files/…`; the annotated download rebuilt
   from rows.
5. **The scan.** Rows instead of the file; the 10-second budget.
6. **The page.** The timeline from conversations and sessions; your messages in the background;
   Claude's replies and file links on demand; the same session flags on each day a session touches.
7. **Tests and measurements.** Every step tested as in the screen-flow plan; then §9 measured
   locally and, after a deployment, on AWS, and written up as an analysis.

Existing tests that read the downloaded export, the flags table, or a flag drawn on only one day of
a session crossing midnight will need changes. The list comes to the user for approval before
coding.

## 11. Questions for the user

**Answered on 2026-10-06:** Q1 (withdrawn: flags move onto the message rows, §3); Q2 (the upload is
deleted once processed, §7). The session files of the previous draft are dropped (the user).

**Still open:**

- **Q3 — Opening a web page or SVG that Claude made.** Show it as text (safe, but a web page shows
  as code), or let the browser draw it? Drawn, it runs whatever the file contains. It would be
  served from the file store's own address, not the app's, so it couldn't reach your sign-in. But
  it is still running code from a conversation in your browser. Proposed: draw SVGs, show web pages
  and code as text, with a "download" link for both.

## Self-critique log

### C1 [OPEN]: Two files of a batch processed at once can lose each other's messages
Present in the code built on 2026-10-05 as well. **Mitigation in plan:** versioned conditional
writes with a retry ([§7 (line 112)](2026-10-06-load-only-what-the-page-shows.md#L112)). **Open:** until
this plan is built, a batch whose files share conversations can lose added messages on AWS. Trigger:
this plan's step 2.

### C2 [RESOLVED]: Your messages in one file would race too
**Resolution:** one database row per message ([§3 (line 32)](2026-10-06-load-only-what-the-page-shows.md#L32)).

### C3 [OPEN]: The estimates are not measurements
§9's figures, other than those marked measured, are estimates on stated assumptions. **Open:** step 7
measures them.

### C4 [RESOLVED]: The first estimates table mixed kinds of numbers
**Resolution:** one table, the same four columns, absolute times ([§9 (line 140)](2026-10-06-load-only-what-the-page-shows.md#L140)).

### C5 [RESOLVED]: Files and rows both holding messages
**Resolution:** rows hold every message's text; the session files are dropped, and tool calls,
tool results and thinking are not kept (the user, 2026-10-06). Only files of §4 are stored
([§4 (line 59)](2026-10-06-load-only-what-the-page-shows.md#L59)).

### C6 [RESOLVED]: The Calendar needed messages only to split a session's flags by day
**Resolution:** a session's flags show on every day it touches; the counts are stored with the
session ([§6 (line 102)](2026-10-06-load-only-what-the-page-shows.md#L102)).

### C7 [OPEN]: Deleting the upload loses what isn't kept, for good
Tool calls, tool results and thinking are gone once the upload is deleted, and the annotated
download can no longer reproduce the export. **Mitigation in plan:** the user decided this; the
download still uploads again with its flags (§5). **Open:** if a later plan needs the tool output
(for example to classify messages with it), the user must upload the original exports again.

### C8 [OPEN]: Writes to one user's rows may be rate-limited
Every row of a user shares one key (the user), and DynamoDB limits how fast one key's rows can be
written (about 1,000 small writes a second per partition, from memory, not checked; it splits busy
partitions over time). Processing writes about 4,800 rows for this export. **Mitigation in plan:**
batched writes with retries of whatever DynamoDB returns unprocessed. **Open:** if processing on AWS
measures much slower than §9's 10 s, key message rows by user and conversation instead (one query
per conversation to load them). Trigger: step 7's AWS measurement.

### C9 [OPEN]: A file Claude changed by running a command is kept at its last replayable version
**Mitigation in plan:** such files are marked "may have been changed later" (§4). **Open:** if the
user finds marked files wrong often, drop the replay and keep only the first version, or none.
