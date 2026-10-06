# Load only what the page shows

**Status:** proposed 2026-10-06, not approved. Questions for the user are in §10.

## 1. The problem

To open the timeline, the server rebuilds the user's whole export as one file and the page downloads
it: 63.6 MB for the user's `real-flags.json`. Every scan request re-reads and re-parses that same
file to reach five conversations. The user's question (2026-10-06) was why the page doesn't load
rows from the database instead. It can't today, because messages are stored only inside the
uploaded file. The migration plan decided not to store sessions because they could be "recomputed
from message timestamps already in DynamoDB"
([migration plan, line 81](2026-09-09-rust-aws-backend-migration.md#L81)). Message times never went
into DynamoDB, so the page recomputes everything from the downloaded file.

## 2. What the page needs, measured on `real-flags.json`

| What | Size | Who needs it |
|---|---|---|
| The whole export | 61.8 MB (39.9 MB of tool results, 5.4 MB tool calls, 7.6 MB text, 0.7 MB thinking) | only the annotated download |
| Conversation records (117) | 73 KB, already in the database | every tab |
| Sessions (300; median 10 messages, largest 112) | about 60 KB as rows | Calendar, Conversations, Analytics |
| Your own messages (2,255): text, time, conversation | text 0.41 MB; largest message 5.7 KB, median 121 bytes | Review, Analytics, the scan |
| The text of Claude's reply after each of your messages | 5.8 MB | Review, only with "Show Claude's replies" on |

The Conversations tab is drawn from sessions alone: it shows counts, days, active time, flag icons
and the table of sessions, and no message text. The Calendar also needs each flagged message's time,
because a session crossing midnight shows each day's flags on that day's part; your messages' rows
carry those times.

## 3. Where everything is kept

| Data | Store | Shape |
|---|---|---|
| Conversation records | DynamoDB `Conversations`, `CONV#…` (exists) | unchanged, plus a version number (§6) |
| **Sessions** | DynamoDB `Conversations`, new `SESS#{conversation}#{n}` rows | conversation, start, end, message count, your-message count |
| **Your messages** | DynamoDB `Conversations`, new `MSG#{conversation}#{message}` rows | conversation, session number, time, text (capped, §5) |
| Flags | DynamoDB `MessageFlags` (exists) | unchanged; the automatic and your own flags stay apart (migration plan §4.1) |
| **A session's messages, all of them** | S3, new `sessions/{user}/{conversation}/{n}.json` | the session's messages as in the export; read for Claude's replies and for the annotated download |
| The uploaded file | S3 `raw/…` (exists) | kept (Q2) |

**Why one file per session, not per conversation** (the user asked to consider it):

- **Smaller pieces.** A session file is 111 KB at the median and 1.7 MB at most; a conversation
  file is 190 KB at the median and 7.4 MB at most. Review shows Claude's replies for the 50 rows on
  its current page, which fall within a few sessions, so it loads only those sessions' files.
- **A later export changes only the end.** A later export adds only messages timed outside a
  conversation's stored range (the user's rule, Q18 of the screen-flow plan). So it extends the
  last session, adds new ones, or, rarely, extends or adds before the first. Only those session
  files are rewritten; with conversation files, any addition would rewrite the whole file, up to
  7.4 MB.
- **The cost is more files:** 300 instead of 117 for this export. That means more writes at upload
  (cost in §8) and more reads for the annotated download, which reads them all.

Sessions are cut by the existing rule, a pause of 15 minutes or more. The server already has a
port of it, `build_blocks` ([sessions.rs](../../backend/timeline-core/src/sessions.rs)), which no
route uses today; this plan uses it. Which local day a session is drawn on stays in the browser,
which knows the viewer's time zone (migration plan §4.3).

## 4. What the page loads

| Request | Returns | Size here |
|---|---|---|
| `GET /conversations` (exists) | conversation records | 73 KB |
| `GET /sessions` (new) | every session row | ~60 KB |
| `GET /messages` (new) | every one of your messages: conversation, session, time, text, automatic and your own flags, and its flag handle (the proof each flag save sends back, now issued here instead of with the download) | ~0.8 MB |

From these three the page draws the Calendar, Conversations, Review and Analytics, as it does now
from the downloaded file. `core/export-format.js`'s parsing is replaced by building
`state.conversations`, `state.messages`, `state.humanMessages` and `state.blocks` from the three
answers, so the views keep their current inputs. The loading modal shows real steps ("Loading your
conversations… your sessions… your messages") with a percentage across the three. Each answer is a
single database query, paged through if it passes DynamoDB's 1 MB per call; paging also fixes the
silent truncation of `list_for_user` noted in the screen-flow analysis.

**Loaded only when asked:**

- **Claude's replies.** When "Show Claude's replies" is on, Review loads the session files behind
  the rows on its current page (`GET /sessions/{conversation}/{n}`) and keeps them for the visit.
- **The annotated download.** "Download annotated conversations.json" asks the server to build the
  file from the session files, and downloads it. It is the only thing that builds the whole export.

## 5. Processing an upload

After the file is parsed (once, as now), processing:

1. cuts each new or extended conversation into sessions with `build_blocks`;
2. writes each new or changed session's file, then its row;
3. writes a row for each of your new messages. The text is capped at 100 KB per row (your largest is
   5.7 KB; DynamoDB's limit is 400 KB per row). A longer message is cut, marked `truncated`, and
   Review shows the whole of it from its session file;
4. writes the conversation's record last, with its new version (§6).

The separate added-messages pieces built on 2026-10-05 (`additions/…`) and `conversation_rebuild.rs`
are replaced: a conversation is its sessions.

## 6. Two files processed at once

On AWS each file of a batch is processed by its own run of the processing function, at the same
time. If two of them hold the same conversation, both read its record, both add messages, and the
second write silently replaces the first. That race exists in the code built on 2026-10-05 too
(C1). Fix: the conversation record carries a version number; processing writes it only if the
version is still the one it read (a DynamoDB conditional write), and on a conflict re-reads and
redoes that conversation. Session and message rows are keyed by conversation and position, so a
redo overwrites rather than duplicates.

## 7. The scan

`POST /detect` reads your-message rows instead of the file, and works for up to **10 seconds**
before answering with how many conversations it has done of how many in total (the user's
suggestion, 2026-10-06). The page asks again until all are done, so its bar shows a real
percentage. Ten seconds keeps each request well inside the deployment's 30-second limit
([template.yaml:148](../../infra/template.yaml#L148)).

## 8. Expected load times

**Measured** on this machine on 2026-10-06, with an optimized build (AWS runs optimized builds), the
in-memory stores, and `real-flags.json`:

| Step | Today |
|---|---|
| Upload and processing | 1.1 s |
| Rebuilding the timeline download | 1.0 s |
| Downloading it (63.6 MB) on the same machine | 0.13 s |
| Parsing it in the browser (in Node) | 0.30 s |
| Each scan request (5 conversations), 24 needed | 0.6–0.7 s each, about 15 s in all |
| `GET /conversations` | 2 ms |
| Parsing your messages only (0.59 MB, in Node) | 3 ms |

**Estimated.** Neither column has been run, and AWS has not been measured at all. Network times
assume a 100 Mbit/s connection; the user's real speed through VS Code's forwarding is not known.

| | Today | After this plan | Basis of the estimate |
|---|---|---|---|
| **Opening the timeline** | ≈ 6.5 s: 1.0 s rebuild + 5.1 s for 63.6 MB at 100 Mbit/s + 0.3 s parse | ≈ 0.2 s locally; 0.5–1.5 s on AWS | ~1 MB over three requests (0.1 s at 100 Mbit/s); DynamoDB queries of a few pages each, tens of ms per page; the flags lookup is the slowest part (Q1) |
| **The scan, whole export** | ≈ 15 s (optimized), ≈ 48 s (debug, measured earlier) | under 2 s, in one request | the file parse that took ~0.6 s of each request is gone; scanning 2,255 short messages takes far less |
| **Upload processing** | 1.1 s locally | +1–3 s on AWS | 300 session files (S3 writes, ~20–30 ms each, 10 at a time) and ~2,555 rows (25 per DynamoDB batch, ~100 batches) |
| **Review with Claude's replies on** | already loaded | 0.1–1 s per page of 50 rows | a few session files, 0.3–3 MB |
| **The annotated download** | ≈ 6.5 s | about the same | still builds and sends the whole export, from session files instead of the upload |

**Cost per upload of this export** (AWS's published us-east-1 prices, from memory, not checked
today): S3 writes 300 × $0.005/1,000 ≈ $0.0015; DynamoDB writes ≈ 3,000 write units ≈ $0.004;
storage of the session files ≈ $0.0014 a month. Under a cent per upload.

## 9. Order of work

1. **Storage.** Session and message rows (core types and ports, in-memory and DynamoDB adapters)
   and session files; paging through queries; the conversation record's version.
2. **Processing.** Writing them, with the versioned write and its retry (§5, §6), replacing the
   additions and `conversation_rebuild.rs`.
3. **Routes.** `GET /sessions`, `GET /messages` (with flags and handles),
   `GET /sessions/{conversation}/{n}`; the annotated download built from session files.
4. **The scan.** Rows instead of the file; the 10-second budget.
5. **The page.** Loading the three answers into the views' existing state; real steps in the
   modal; Claude's replies on demand; the annotated download through the server.
6. **Tests and measurements.** Every step tested as in the screen-flow plan, then this plan's §8
   measured again, locally, and on AWS after a deployment, and written up as an analysis.

Existing tests that read the downloaded export (the annotated-export tests, flag-handle tests that
read handles from `GET /export`) will need changes; the list comes to the user for approval before
coding, as before.

## 10. Questions for the user

- **Q1 — Reading every flag at once.** Flags are kept per conversation (their table's key starts
  with the user and conversation), so loading all of them is one query per conversation: 117 here,
  run 10 at a time, about 0.3–1 s on AWS by estimate. Alternatively, add an index to the flags table
  keyed by user alone, making it one query. That is a change to the deployed table (an index added
  to it), cheap but permanent. Which?
- **Q2 — The uploaded file after processing.** Keep it (a second copy of everything, about $0.0014
  a month for this export, and the original for re-processing if this format changes again), or
  delete it once its sessions are written?

## Self-critique log

### C1 [OPEN]: Two files of a batch processed at once can lose each other's messages
Present in the code built on 2026-10-05 as well: processing reads a conversation's record, adds
messages, and writes it back, so two simultaneous runs over the same conversation can overwrite
each other. **Mitigation in plan:** versioned conditional writes with a retry
([§6 (line 97)](2026-10-06-load-only-what-the-page-shows.md#L97)). **Open:** until this plan is
built, a batch whose files share conversations can lose added messages on AWS. Trigger: this plan's
step 2.

### C2 [RESOLVED]: Your messages in one file would race too
The first draft kept your messages' text in one file per user, but two processing runs would each
rewrite it. **Resolution:** one database row per message
([§3 (line 31)](2026-10-06-load-only-what-the-page-shows.md#L31)), safe because your largest message
is 5.7 KB against a 400 KB row limit, with a cap and fallback for an outsized one (§5).

### C3 [OPEN]: The estimates are not measurements
§8's "after" column and every AWS figure are estimates. **Open:** step 6 measures them; the plan is
judged by those numbers, not these.
