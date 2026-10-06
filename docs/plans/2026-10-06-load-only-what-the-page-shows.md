# Load only what the page shows

**Status:** proposed 2026-10-06, revised the same day with the user's answers; not approved.

## 1. The problem

To open the timeline, the server rebuilds the user's whole export as one file and the page downloads
it: 63.6 MB for the user's `real-flags.json`. Every scan request re-reads and re-parses that same
file to reach five conversations. The user's question (2026-10-06) was why the page doesn't load
rows from the database instead. It can't today, because messages are stored only inside the
uploaded file. The migration plan decided not to store sessions because they could be "recomputed
from message timestamps already in DynamoDB"
([migration plan, line 81](2026-09-09-rust-aws-backend-migration.md#L81)). Message times never went
into DynamoDB, so the page recomputes everything from the downloaded file.

## 2. What each part of the page needs, measured on `real-flags.json`

| What | Size | Who needs it |
|---|---|---|
| The whole export | 61.8 MB: 39.9 MB tool results, 5.4 MB tool calls, 7.6 MB text, 0.7 MB thinking | only the annotated download |
| Conversation records (117) | 73 KB, already in the database | every tab |
| Sessions (300; median 10 messages, largest 112) | about 70 KB as rows, with their flag counts | **Calendar and Conversations, alone** |
| Your messages (2,255) | text 0.41 MB; largest 5.7 KB, median 121 bytes | Review, Analytics, the scan |
| Claude's messages (2,227) | text 5.78 MB; largest 18 KB, median 2.2 KB | Review, only with "Show Claude's replies" on |

The Calendar and the Conversations tab need no messages. Each session's flag counts are stored with
the session. A session that crosses midnight shows the same flags on each day it touches (the user's
decision, 2026-10-06), so no message times are needed to split them by day. Today the Calendar shows
on each day only the flags of messages sent that day; that changes.

## 3. Where everything is kept

| Data | Store | Shape |
|---|---|---|
| Conversation records | DynamoDB `Conversations`, `CONV#…` (exists) | unchanged, plus a version number (§6) |
| **Sessions** | `Conversations`, new `SESS#{conversation}#{n}` rows | conversation, start, end, message counts, and flag counts (§5) |
| **Messages, yours and Claude's** | `Conversations`, new `MSG#{conversation}#{message}` rows | conversation, session, sender, time, text (no tool output) |
| Flags | DynamoDB `MessageFlags`, **re-keyed by user** (§4) | automatic and your own flags still apart (migration plan §4.1) |
| **The full archive of each session** | S3, new `sessions/{user}/{conversation}/{n}.json` | every message as in the export, tool output included |
| The uploaded file | S3 `raw/…` | **deleted once processed** (the user, Q2) |

**Why files as well as rows.** The rows hold what the page shows: who, when, and text. The rest of
the export (tool results, tool calls, attachments, thinking: 54 MB of the 62) is shown nowhere, but
"Download annotated conversations.json" promises a file that is your export with your flags written
into it, which can be uploaded again. With the original upload deleted, the session files are the
only full copy, and only that download reads them. One file per session rather than per conversation
because a later export usually changes only a conversation's last session, so only that file is
rewritten (a conversation file is up to 7.4 MB here; a session file up to 1.7 MB).

Sessions are cut by the existing rule, a pause of 15 minutes or more, using the server's existing
port of it, `build_blocks` ([sessions.rs](../../backend/timeline-core/src/sessions.rs)), which no
route uses today. Which local day a session is drawn on stays in the browser, which knows the
viewer's time zone (migration plan §4.3).

## 4. What the page loads

| When | Request | Returns | Size here |
|---|---|---|---|
| Opening the timeline | `GET /conversations` (exists) | conversation records | 73 KB |
| | `GET /sessions` (new) | every session, with its flag counts | ~70 KB |
| Right after, in the background | `GET /messages` (new) | your messages: conversation, session, time, text, automatic and your own flags, and each one's flag handle (now issued here) | ~0.8 MB |
| With "Show Claude's replies" on | `GET /messages?replies` (new) | Claude's messages' text | 5.8 MB, loaded once |
| "Download annotated…" | `GET /export` (exists) | built from the session files | 63.6 MB |

The timeline shows as soon as the first two answer; the loading modal counts the two. Review and
Analytics need your messages: if they are opened before that load finishes, they say "Loading your
messages…" until it does. Each request is one database query (paged through past DynamoDB's 1 MB
per call, which also fixes the silent cut-off of `list_for_user` noted in the screen-flow analysis).

**The flags table re-keyed by user.** Its key is now user-and-conversation, so reading all of a
user's flags takes one query per conversation (117 here). Its data was cleared on 2026-10-06, so it
can be replaced by a table keyed by user, with conversation-and-message as the sort key, at no
cost: one query reads every flag. (This replaces the earlier draft's question Q1.)

## 5. Session flag counts

The page's show switches combine flags three ways (automatic only, yours only, both with yours
winning), so each session row stores, for each of the three flags (critical, angry, ALL-CAPS), the
count under each combination: nine numbers. They change whenever a flag does:

- **A flag save** (you tick a box or press Approve) recounts that message's session: it reads the
  session's flags (one query; a session has at most 112 messages here) and rewrites the row's
  counts, in the same request.
- **The scan** recounts each session it finishes.

The page then draws the Calendar and the Conversations tab from these counts, and no longer counts
flags itself for those two (`attachFlags` remains for Review and Analytics).

## 6. Processing an upload

After the file is parsed (once, as now), processing:

1. cuts each new or extended conversation into sessions with `build_blocks`;
2. writes each new or changed session's file, then the rows of its new messages, then its row;
3. writes the conversation's record last, with its new version;
4. deletes the uploaded file once everything is written. A failed attempt leaves it, so a retry
   still has it.

On AWS each file of a batch is processed at the same time as the others. If two hold the same
conversation, both would read its record, add messages and write it back, the second write silently
replacing the first: a race that exists in the code built on 2026-10-05 too (C1). Processing
therefore writes the record only if its version is still the one it read (a DynamoDB conditional
write), and on a conflict re-reads and redoes that conversation. Session and message rows are keyed
by conversation and position, so a redo overwrites rather than duplicates.

The separate added-messages pieces built on 2026-10-05 (`additions/…`) and `conversation_rebuild.rs`
are replaced.

## 7. The scan

`POST /detect` reads your message rows instead of the file, and works for up to **10 seconds**
before answering with how many conversations it has done of how many in total (the user's
suggestion, 2026-10-06). The page asks again until all are done, so its bar shows a real
percentage. Ten seconds keeps each request well inside the deployment's 30-second limit
([template.yaml:148](../../infra/template.yaml#L148)).

## 8. Expected times

**Measured** on this machine on 2026-10-06, optimized build, in-memory stores, `real-flags.json`:
upload and processing 1.1 s; rebuilding the timeline download 1.0 s; downloading it on the same
machine 0.13 s; parsing it (in Node) 0.30 s; each of the 24 scan requests 0.6–0.7 s; the conversation
list 2 ms.

**Every other figure is an estimate.** Assumptions: the browser reaches the server at 100 Mbit/s
(the user's real speed through VS Code's forwarding isn't known); AWS runs the processing and API
functions with 512 MB of memory, which Lambda pairs with roughly 0.3 of a processor (from AWS's
documentation as I remember it, not checked), so work that takes 1 s here takes about 3.5 s there;
DynamoDB and S3 calls take 10–30 ms each.

| | Today, local | Today, AWS | After, local | After, AWS |
|---|---|---|---|---|
| **Opening the timeline** | 6.4 s (measured parts + 5.1 s to send 63.6 MB) | 9 s | 0.1 s | 0.5 s |
| **…and your messages, in the background** | (included above) | (included above) | 0.2 s | 1 s |
| **The scan** | 15 s (measured) | 50 s | under 1 s | 3 s |
| **Processing an upload** | 1.1 s (measured) | 5 s | 1.5 s | 8 s |
| **Showing Claude's replies** | already loaded | already loaded | 0.5 s once | 1.5 s once |
| **The annotated download** | 6.4 s | 9 s | 6.5 s | 9.5 s |
| **Saving one flag** | under 0.1 s | 0.2 s | under 0.1 s | 0.3 s |

Processing gets slower on AWS: it writes about 4,500 message rows (25 per DynamoDB batch, so about
180 batches), 300 session rows and 300 session files.

**Cost per upload of this export** (AWS's published us-east-1 prices, from memory, not checked):
DynamoDB writes of about 8,000 write units, roughly $0.005–0.01; S3 writes 300 × $0.005/1,000 ≈
$0.0015; storage of the session files about $0.0014 a month, the same as the deleted upload took.

## 9. Order of work

1. **Storage.** Session and message rows (core types and ports, in-memory and DynamoDB adapters),
   session files, paged queries, the conversation record's version, and the flags table re-keyed by
   user (a new table in the template; nothing to move).
2. **Processing.** Sessions, messages and files written with the versioned write and its retry;
   the upload deleted when done; the additions and `conversation_rebuild.rs` replaced.
3. **Flag counts.** Session counts kept current by flag saves and the scan.
4. **Routes.** `GET /sessions`, `GET /messages` (with flags and handles, and Claude's replies on
   request); the annotated download built from session files.
5. **The scan.** Rows instead of the file; the 10-second budget.
6. **The page.** The timeline from conversations and sessions; your messages loaded in the
   background for Review and Analytics; Claude's replies on demand; the same session flags on each
   day a session touches.
7. **Tests and measurements.** Every step tested as in the screen-flow plan; then §8 measured
   locally and, after a deployment, on AWS, and written up as an analysis.

Existing tests that read the downloaded export, or that check a flag drawn on only one day of a
session crossing midnight, will need changes. The list comes to the user for approval before coding.

## 10. Questions for the user

**Answered on 2026-10-06:** Q1 (withdrawn: it assumed the flags table stays keyed by conversation;
it is re-keyed by user instead, §4); Q2 (the original upload is deleted once processed, §6).

**Still open:** none.

## Self-critique log

### C1 [OPEN]: Two files of a batch processed at once can lose each other's messages
Present in the code built on 2026-10-05 as well. **Mitigation in plan:** versioned conditional
writes with a retry ([§6 (line 89)](2026-10-06-load-only-what-the-page-shows.md#L89)). **Open:** until
this plan is built, a batch whose files share conversations can lose added messages on AWS. Trigger:
this plan's step 2.

### C2 [RESOLVED]: Your messages in one file would race too
The first draft kept your messages' text in one file per user, which two processing runs would each
rewrite. **Resolution:** one database row per message
([§3 (line 31)](2026-10-06-load-only-what-the-page-shows.md#L31)).

### C3 [OPEN]: The estimates are not measurements
§8's figures, other than those marked measured, are estimates built on stated assumptions.
**Open:** step 7 measures them; the plan is judged by those numbers.

### C4 [RESOLVED]: The first estimates table mixed kinds of numbers
It gave one figure for today but two for after, and a change rather than a time for processing.
**Resolution:** one table, the same four columns, absolute times
([§8 (line 117)](2026-10-06-load-only-what-the-page-shows.md#L117)).

### C5 [RESOLVED]: Files and rows both holding messages, without saying why
The user asked why keep files if the database holds the messages. **Resolution:** rows hold every
message's text (all of it fits: the longest is 18 KB); files hold only what rows don't, as the full
archive for the annotated download ([§3 (line 31)](2026-10-06-load-only-what-the-page-shows.md#L31)).

### C6 [RESOLVED]: The Calendar needed messages only to split a session's flags by day
**Resolution:** the user decided a session's flags show on every day it touches; the counts are
stored with the session ([§5 (line 75)](2026-10-06-load-only-what-the-page-shows.md#L75)).

### C7 [OPEN]: Deleting the upload makes the session files the only full copy
If writing the session files has a fault, nothing is left to rebuild from. **Mitigation in plan:**
the upload is deleted only after every file and row is written (§6), and the tests rebuild the
annotated download from the files and compare it with the original. **Open:** revisit if the
download from files ever differs from the original in the step 7 measurements.
