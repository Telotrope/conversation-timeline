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
| Citations (2,446: a span of a reply's text and the web address it came from) | 0.52 MB | **yes**, shown in Review (§4c) |
| Each message's link to the message it answers (`parent_message_uuid`) | 0.17 MB | **yes**: used to remove replaced branches (§4d) |
| Your attachments (8, with their text extracted, up to 32 KB) | < 0.1 MB | **yes, as files** (§4) |
| Files Claude wrote whose text is in the export (24 created files, 16 widgets) | ~1.5 MB | **yes, as files** (§4) |
| Your uploaded files (90 references) and files Claude made by running scripts (118: Word, PNG, PowerPoint…) | names only; their contents are not in the export | names only |

## 3. Where everything is kept

| Data | Store | Shape |
|---|---|---|
| Conversation records | DynamoDB `Conversations`, `CONV#…` (exists) | unchanged, plus a version number (§7) |
| **Sessions** | `Conversations`, new `SESS#{conversation}#{n}` rows | conversation, start, end, message counts, flag counts (§6) |
| **Messages**, and the notes of §4d | `Conversations`, new `MSG#{conversation}#{message}` rows | conversation, session, sender (or "note"), time, the message it answers, and its content in order: text pieces with their citations, and markers for the files it presented, where it presented them (§4). For yours, **its flags**: the automatic ones and your own as separate attributes |
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

**Where the files appear.** Claude's export doesn't link a file to words in the reply (the reply
text names the file in only 8 of the 142 files presented here). What it does record is where in the
reply each file was presented: the presenting step sits between two paragraphs of the reply, with
the file's name and type. So the files appear where Claude put them:

- **In Review, inline in Claude's reply,** between the paragraphs where it presented them, as a card
  with the file's name; a file whose contents aren't in the export says so on its card. Review shows
  Claude's replies only with "Show Claude's replies" on.
- **In the Conversations tab,** the open conversation lists every file Claude presented in it, each
  linking to the reply that presented it in Review. So files can be found without reading replies.

Opening a card (`GET /files/{conversation}/{message}/{name}` answers with a short-lived download
address) shows the file on the page, **never running any code it contains** (the user, Q3), with a
download link beside it:

| Kind of file | Shown as | How |
|---|---|---|
| SVG | the drawing | an `<img>` element: browsers don't run scripts inside an SVG shown this way |
| Web page (HTML), interactive widget | the page as laid out, scripts off; "Show source" switches to its text | a sandboxed `<iframe>` (the `sandbox` attribute with no permissions: the browser runs no scripts, opens no pop-ups, submits no forms). Built into browsers; no library. A widget that draws itself with scripts shows only its static parts |
| Markdown | formatted text | the page's own `renderMarkdownLite` ([markup.js](../../frontend/ui/render/markup.js)), already used for messages |
| Code (Python, JavaScript, Apps Script…) | text with syntax colouring | **highlight.js** (BSD-3-Clause, to be confirmed from its licence file when vendored): it only colours text and runs nothing from the file. Vendored under `vendor/` like `oidc-client-ts` |
| Your attachments | their text | as text |

Considered and not chosen: converting web pages to Markdown with **Turndown** (MIT) and showing that.
It reads well for text-heavy pages but drops layout, tables' styling and drawings, which the
sandboxed frame keeps; it stays an option if the frame turns out awkward.

## 4c. Citations

A citation marks a span of a reply's text (its start and end positions) and the web address it came
from. Review shows each cited span with a small numbered link after it to the address; the address
opens in a new tab. The positions refer to the raw text, before Markdown formatting, so the markers
are placed before the text is formatted. Citations are kept in the message rows with their text
pieces, and are part of the annotated download.

## 4d. Replaced branches

When you edit a message or send it again, claude.ai keeps the earlier version too: the conversation
becomes a tree, every message naming the one it answers. The export holds every branch. The user
wants the record to be one consistent narrative (2026-10-06): a replaced branch is removed, and a
note stands in its place.

**Which branch is kept.** The path from the conversation's start to its most recent message; every
branch off that path was replaced. This is not quite "the newer reply wins", which the data shows
would sometimes keep the wrong one. In "Starting a government contracting business" the user's
message at 16:30 on 21 June was answered, and the conversation went on from that answer for 117
messages over the following days; a resend of the same message at 16:38 got a reply and went
nowhere. "The newer reply wins" would keep the dead end and drop the 117 messages. The path to
the latest message keeps them. (That the 16:38 message was a resend is inferred from its identical
text; the export doesn't say why it was sent.)

**Measured on `real-flags.json`:** 38 messages are removed, in 32 replaced branches. All 32 were
started by the user (none by a retried reply from Claude); each holds at most 4 messages, sent
within 15 seconds of each other.

**The note.** Each replaced branch becomes one note row in the conversation: "A branch here was
replaced: N messages, from HH:MM to HH:MM". It sits where the branch began, and its start and end
count as activity when sessions are cut, so removing a branch can't split a session that the user
was in fact working through (the user's suggestion). Measured: removing the branches alone cuts one
session in two (300 sessions become 299, with a false gap inside one); with the notes, the sessions
are the same 300 as before. Notes are not messages: they count in no message total, flag count or
analysis. Review shows them as a thin line between messages. The annotated download leaves them
out, since the export format has nothing like them.

**Where it happens.** In processing, on the server, as one pure function in `timeline-core`
(`prune_replaced_branches`: a conversation in, the kept path and its notes out), before the existing
retried-message clean-up (`dedup_chat_messages`), which still removes resends that the tree does not
show as branches.

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

## 7b. Slimming the file in the browser before sending it

Today the page sends the user's file unchanged, as soon as Upload is pressed: 63.5 MB, 26 s on the
user's connection (measured). Most of it is never kept (§2). So the page keeps only what the plan
uses and sends that, compressed (the user's suggestion, 2026-10-06):

- **Kept:** each conversation's id, name and times; each message's id, the message it answers,
  sender, time, attachments, file references and review (`_claude_timeline_user`, so a re-uploaded
  annotated file keeps your flags); its text pieces with their citations; and the tool calls that
  write or present files (`create_file`, `str_replace`, `visualize:show_widget`, `present_files`;
  their input only), in their place among the text pieces.
- **Dropped:** tool results, every other tool call, thinking, each piece's own timestamps, and the
  message-level `text` field. That field is not a summary: it is the message's text pieces joined
  together, with the placeholder "This block is not supported on your current device yet." wherever
  a tool call or file sat, which is why it is longer than the pieces. Nothing in this project reads
  it (the server and page both take text from the pieces).
- **The "may have been changed later" mark** (§4, C9) needs the shell commands, which are dropped.
  So the page looks for later commands naming each created file while it still has the whole file,
  and adds the mark to that file's `create_file` call before sending.
- **Compressed** with the browser's built-in gzip compression (`CompressionStream`); the server
  decompresses during processing with **flate2** (MIT or Apache-2.0; to be confirmed from its
  licence file when added).

Measured on 2026-10-06 in Node on this machine, with `real-flags.json`:

| | Size | Time |
|---|---|---|
| The file as it is | 63.5 MB | |
| Slimmed | 9.3 MB | parse 0.3–0.7 s, slim 0.1 s |
| Slimmed and compressed (what is sent) | 2.8 MB | compress 0.3 s |

At the user's measured 2.5 MB/s, sending 2.8 MB takes about 1 s instead of 26 s. The server then
parses 9.3 MB instead of 63.5 MB, so processing gets faster too. Browsers may parse more slowly
than Node; C11.

## 8. The scan

`POST /detect` reads your message rows instead of the file, and works for up to **10 seconds**
before answering with how many conversations it has done of how many in total (the user's
suggestion). The page asks again until all are done, so its bar shows a real percentage. Ten seconds
keeps each request well inside the deployment's 30-second limit
([template.yaml:148](../../infra/template.yaml#L148)).

## 9. Expected times

**What is measured.** On AWS, from the deployment checks of 2026-10-02 and the activity run of
2026-10-05 ([analysis](../analysis/2026-10-05-activity-recording-aws-run.md#L40)), with the user's
63.5 MB export and the user's own connection: sending the file to storage 26.0 s; processing on the
server 5.7–6.4 s; the scan 24 requests of about 3.2 s, about 80 s in all; rebuilding the timeline
download 12.3 s; downloading it 25.1 s. The rebuild and download ran while an upload was also
being sent, so on their own they may be faster. Locally, on 2026-10-06 with an optimized build:
processing 1.1 s; rebuilding the download 1.0 s; each scan request 0.6–0.7 s; parsing the download
(in Node) 0.3 s.

**Assumptions for the estimates.** The user's connection to AWS moves about 2.5 MB/s each way (from
the 25.1 s and 26.0 s above). Locally, through VS Code's forwarding, its speed is not known, so the
local column assumes the same. Server work on AWS takes about 5 times as long as on this machine
(the measured ratio: scan requests 3.2 s against 0.65 s, processing 6 s against 1.1 s). DynamoDB and
S3 calls take 10–30 ms each.

| What you wait for | Today, local | Today, AWS | After, local | After, AWS |
|---|---|---|---|---|
| **Preparing the file in the browser** (§7b) | none | none | 1.5 s | 1.5 s |
| **Sending the file** | 26 s (63.5 MB) | 26 s (measured) | 1 s (2.8 MB) | 1 s |
| **Processing** (from the file arriving to Describe) | 1 s (measured) | 6 s (measured) | 0.5 s | 6 s |
| **Scanning, if ticked** | 15 s (measured) | 80 s (measured) | 1 s | 5 s |
| **Opening the timeline** | 27 s | 37 s (measured, during an upload) | 0.1 s | 1 s |
| …your messages, in the background | (included above) | (included above) | 0.5 s | 2 s |
| **Showing Claude's replies** (5.8 MB) | already loaded | already loaded | 3 s | 4 s |
| **The annotated download** | 27 s | 37 s | 3 s | 5 s |

So the wait from pressing Upload to Describe, with the scan ticked, goes from about 112 s on AWS
today to about 14 s.

Processing parses an eighth as much as today, then writes about 4,800 rows (25 per DynamoDB batch)
and about 50 files; it may be slower if DynamoDB limits the rate of writes to one user's rows (C8).

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
5b. **Slimming the upload** (§7b): in the page, and the server's decompression.
5d. **Replaced branches** (§4d): pruning and notes in processing; notes in sessions and Review.
5c. **Files and citations in Review** (§4, §4c): file cards in place in Claude's replies, the
   Conversations tab's list of a conversation's files, numbered citation links.
6. **The page.** The timeline from conversations and sessions; your messages in the background;
   Claude's replies and file links on demand; the same session flags on each day a session touches.
7. **Tests and measurements.** Every step tested as in the screen-flow plan; then §9 measured
   locally and, after a deployment, on AWS, and written up as an analysis.

Existing tests that read the downloaded export, the flags table, or a flag drawn on only one day of
a session crossing midnight will need changes. The list comes to the user for approval before
coding.

## 11. Questions for the user

**Answered on 2026-10-06:** Q1 (withdrawn: flags move onto the message rows, §3); Q2 (the upload is
deleted once processed, §7); Q3 (SVGs drawn; web pages and code shown without running anything, with
a download link, §4). The session files of the previous draft are dropped (the user).

**Still open:**

- **Q4 — Which branch is kept.** The path to each conversation's latest message (§4d), instead of
  "the newer reply wins", which would have dropped 117 messages of a real conversation in favour of
  a dead end. Agreed?

## Self-critique log

### C1 [OPEN]: Two files of a batch processed at once can lose each other's messages
Present in the code built on 2026-10-05 as well. **Mitigation in plan:** versioned conditional
writes with a retry ([§7 (line 179)](2026-10-06-load-only-what-the-page-shows.md#L179)). **Open:** until
this plan is built, a batch whose files share conversations can lose added messages on AWS. Trigger:
this plan's step 2.

### C2 [RESOLVED]: Your messages in one file would race too
**Resolution:** one database row per message ([§3 (line 34)](2026-10-06-load-only-what-the-page-shows.md#L34)).

### C3 [OPEN]: The estimates are not measurements
§9's figures, other than those marked measured, are estimates on stated assumptions. **Open:** step 7
measures them.

### C4 [RESOLVED]: The first estimates table mixed kinds of numbers
**Resolution:** one table, the same four columns, absolute times ([§9 (line 242)](2026-10-06-load-only-what-the-page-shows.md#L242)).

### C5 [RESOLVED]: Files and rows both holding messages
**Resolution:** rows hold every message's text; the session files are dropped, and tool calls,
tool results and thinking are not kept (the user, 2026-10-06). Only files of §4 are stored
([§4 (line 61)](2026-10-06-load-only-what-the-page-shows.md#L61)).

### C6 [RESOLVED]: The Calendar needed messages only to split a session's flags by day
**Resolution:** a session's flags show on every day it touches; the counts are stored with the
session ([§6 (line 169)](2026-10-06-load-only-what-the-page-shows.md#L169)).

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

### C10 [RESOLVED]: The first AWS estimates ignored measurements that existed
They put opening the timeline on AWS at 9 s and processing at 5 s, from a guessed slow-down. The
activity run of 2026-10-05 had measured 37 s and 6 s, and sending the file, 26 s, was missing
altogether. **Resolution:** §9 now starts from those measurements, lists what you wait for step by
step, and derives the slow-down from them ([§9 (line 242)](2026-10-06-load-only-what-the-page-shows.md#L242)).

### C11 [OPEN]: Slimming is measured in Node, not in a browser
Parsing the 63.5 MB file took 0.3–0.7 s in Node on this machine; a browser on a slower computer may
take several seconds, and holds the whole file in memory meanwhile, as it already does today.
**Open:** step 7 measures it in the user's browser. Trigger: if preparing takes over 10 s, slim on
the server instead (sending the full file again).

### C12 [OPEN]: A later export can continue a branch that was pruned, or prune one that was kept
The path is decided per upload. If a later export's newest messages answer a message that an earlier
upload's path did not end on, the branch kept earlier was itself replaced. **Mitigation in plan:**
none yet. **Open:** processing would need to compare each new message's parent with the stored
rows, remove the stored messages after the branch point and add a note: a comparison of stored
messages the user had ruled out for finding new messages (§7), though here only by the parent
link of each new message. Trigger: Q4's answer, and the user's view on this comparison.

