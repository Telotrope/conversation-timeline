# Load only what the page shows

**Status:** approved 2026-10-06, with the changes to existing tests (§10b) and the resumption
tests (§8c). Coding started the same day: the first core types are committed but untested and
unused (§10, "Already done"). To be carried out by another session. Open questions are in §11.

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
| **Messages**, and the notes of §4d | `Conversations`, new `MSG#{conversation}#{time}#{message}` rows: the time in the key puts a session's messages in one unbroken run of keys, read with one range query (§5b) | conversation, session, sender (or "note"), time, the message it answers, and its content in order: text pieces with their citations, and markers for the files it presented, where it presented them (§4). For yours, **its flags**: the automatic ones and your own as separate attributes |
| **Files** | S3, new `files/{user}/{conversation}/{message}/{number}` | the files of §4; numbered within their message, with the name kept on the message row, so no name from an upload ever becomes part of a storage key |
| The `MessageFlags` table | **removed**: flags move onto the message rows | (its data was cleared on 2026-10-06) |
| The uploaded file | S3 `raw/…` | **deleted once processed** (the user) |
| **Analysis results** | `Conversations`, new `ANALYSIS#{name}#{options}` rows | the numbers of one analysis, and the data version they were computed from (§5c) |
| **Data version** | an attribute on the user's existing record | a number raised by every upload, flag save and scan (§5c) |

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

Opening a card (`GET /files/{conversation}/{message}/{number}` answers with a short-lived download
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

**The note.** Each replaced branch becomes one note row in the conversation. Review shows it as a
message in its own row, worded: "An earlier branch of this conversation was pruned here: N messages
(W words not repeated below) from HH:MM to HH:MM, replaced by the message below." (the user,
2026-10-06; not just a line). When every message in the branch is repeated on the kept path, it
reads instead: "An earlier copy of the message below was pruned here (sent HH:MM)." It sits where the branch began, and its start and end
count as activity when sessions are cut, so removing a branch can't split a session that the user
was in fact working through (the user's suggestion). Measured: removing the branches alone cuts one
session in two (300 sessions become 299, with a false gap inside one); with the notes, the sessions
are the same 300 as before. Notes are not messages: they count in no message total, flag count or
analysis. The annotated download leaves them out, since the export format has nothing like them.

**Measured on the 32 replaced branches here:** every one is one or two messages of yours, and the
three Claude replies inside them have no text (their answers apparently never arrived, so these look
like failed sends that were resent; inferred, as the export records no failures). 26 of them carry a
review of yours, because this file was saved by this tool's annotated download.

**Branches worth keeping as their own conversation** (the user, 2026-10-06, answering C12 and Q5).
A replaced branch is important when it holds **100 words or more once its repeated messages are
removed**: a message in the branch is repeated when its text is included in the text of a message
from the same sender on the kept path of the same conversation (both compared after collapsing runs
of spaces and line breaks). "Included" covers an identical message, and also a message that was cut
off and then sent again in full, or sent and then extended (the user, 2026-10-06, after reading the
branches: two of them were such cut-off starts). An empty message is included in any message, and
copies of one message within a branch are each included, so neither adds words. The same count
gives the "W words not repeated below" in the note. Measured here: 26 of the 32 branches are
nothing but repeats (0 words left); five keep 1 to 10 words; one is kept, two messages of 155 words
in "C-Corporation banking setup for payroll". The user confirmed (2026-10-06) that this
branch is a real loss: two dictated messages that Claude never answered, after which the user asked
again in a new message, so keeping it is the rule working as intended. An important branch is not reduced to a note:
it is kept as a conversation of its own, named "{name}: earlier branch from {date, time}", holding
the branch's own messages from the branch point on, and linked to the conversation it branched from
(both show the link). It holds only the branch's messages, not the start the two share, so no
message is counted twice on the Calendar or in the analyses. The note in the main conversation
then reads "…was kept as its own conversation", with a link to it.

**A later export that revives a pruned branch** (C12). If a later export's newest messages continue a
branch that an earlier upload had pruned, that branch now holds the latest message, so it becomes
the conversation's path. Processing finds this by looking up each new message's parent among the
stored rows: a parent that isn't at the end of the stored path marks a branch point. What the stored
path held after that point becomes the replaced branch: a note, or its own conversation if it is
important (Q5). The revived branch's earlier messages come from the new export, which holds every
branch.

**Where it happens.** In processing, on the server, as one pure function in `timeline-core`
(`prune_replaced_branches`: a conversation in, the kept path and its notes out), before the existing
retried-message clean-up (`dedup_chat_messages`), which still removes resends that the tree does not
show as branches.

## 4e. Messages whose time is unknown

Today the server refuses a whole file if any message has no `created_at`
([model.rs](../../backend/timeline-core/src/model.rs), `ChatMessage`), so the page's rule for placing
such conversations (`placedBySpan` in [blocks.js](../../frontend/core/blocks.js)) could never run.
The user (2026-10-06) wants such messages accepted, with a sentinel for "time unknown":

- **Reading.** A message with no `created_at`, or `null`, is read with the time unknown. A value
  that is present but isn't a time is still refused, as now: it is a damaged file, not a missing
  fact.
- **The sentinel.** Where a time must be written down (the parsed message's `created_at`, a row's
  key, the annotated download), unknown is written as the zero date, `1970-01-01T00:00:00Z`. Code
  never compares against it directly: `ChatMessage::time()` and `EntryKey::time()` return a
  `MessageTime` (known, or unknown), so no reader can mistake the sentinel for a real time. A real
  message sent at that exact instant would be read as unknown; no Claude conversation is that old.
  The annotated download leaves `created_at` out for such messages, so a round trip keeps them
  unknown.
- **Placing.** A conversation with any message of unknown time is one session from its start to
  its end as its record gives them (the page's rule, moved to the server): where the user put it,
  or the upload's guess. Its timed messages don't place it. Editing its start and end moves the
  session. The session row records that it was placed this way, and reading its messages reads the
  whole conversation rather than a range of times.
- **Counting and filters.** Such messages count in their conversation's message total and in the
  session's counts. A conversation filter finds them; a time span finds them only through their
  session (a span overlapping it); a Calendar day never does, as the page has always behaved
  ("counted but not placed on the timeline").
- **Merging a later file.** A message of unknown time is never added to a known conversation by
  the time-range rule (it can't be outside the range); it is added only when the conversation is
  new. Recorded in the run's log line as "messages of unknown time skipped".
- **The record** gains `untimed`, the count of such messages, so Describe can warn as the
  screen-flow plan's Q19 asked (the warning was never built because the case was impossible).

## 5. What the page loads

**Nothing loads all messages.** The first draft of this plan loaded every message of yours in the
background and every reply of Claude's at once (C13); the user (2026-10-06) asked for messages to
load only as a view shows them. Each view asks the server for what it draws:

| When | Request | Returns | Size here |
|---|---|---|---|
| Opening the timeline | `GET /conversations` (exists) | conversation records | 73 KB |
| | `GET /sessions` (new) | every session, with its flag counts | ~90 KB |
| Opening Review, or changing its filters, search or page | `GET /messages?…` (new, §5b) | one page of 50 of your messages with their flags, the number of pages, and, with "Show Claude's replies" on, the reply that follows each of the 50 | ~15 KB; with replies ~150 KB |
| Opening "Flag rate over time" or "Time of day", or changing their options | `GET /analyses/{name}?…` (new, §5c) | that analysis's numbers | < 10 KB |
| Opening a conversation in the Conversations tab | `GET /conversations/{id}/files` (new) | the files Claude presented in it (§4) | small |
| A file's link in Review | `GET /files/…` (new) | a download address for it | |
| "Download annotated…" | `GET /export` (exists, rebuilt) | conversations from the rows: text, flags and file names | ~6.5 MB |

The Calendar and the Conversations tab are drawn from conversations and sessions alone. A session
that crosses midnight shows the same flags on each day it touches (the user's decision); today the
Calendar shows each day only its own messages' flags. Each request pages through past DynamoDB's
1 MB per call (which also fixes the silent cut-off of `list_for_user` noted in the screen-flow
analysis).

**What changes for the page.** It no longer holds `state.humanMessages` or any message text beyond
the page of Review on screen. A flag checkbox saves as now, then asks for the same page again (and
the sessions, whose counts changed). The Review code that filters, sorts and pages
([review.js:44](../../frontend/ui/views/review.js#L44), `getFilteredHumanMessages`) and two of the
five analyses move to the server; the other three analyses are computed in the page from the
sessions (§5c), and drawing the table and the charts (SVG) stays in the page.

**The annotated download changes.** It no longer reproduces the export, since tool calls, tool
results and thinking are not kept. It holds every conversation's messages as text, with your flags
and the names of their files. It can still be uploaded again: messages of text alone are a valid
export, and uploading it brings back your flags.

## 5b. Finding messages: one filter, shared by every route that reads messages

Review's filters today, as read in [review.js:44-67](../../frontend/ui/views/review.js#L44-L67):

- **where:** one conversation, a time span (from a session, an analysis point, or a Calendar day,
  which the page sends as its local midnight to midnight, since the server doesn't know the
  viewer's time zone), or both, or neither;
- **flag:** all, flagged (any of the three), ALL-CAPS, angry, critical, or "overridden";
- **search:** letters your message's text must contain, ignoring capitals;
- **view:** the show switches (automatic flags, yours, both, or neither), which decide what
  "flagged" means.

"Overridden" in the code (`isOverridden`, [flags.js:28](../../frontend/core/flags.js#L28)) means a
message has a value of yours for any flag, even one that agrees with the automatic flag; that is
the same set as the messages you reviewed (`isReviewed`, line 66). (Claude described it on
2026-10-06 as "your choice differs from the automatic flag"; that was wrong.)

**The shared code.** Three pieces, used by every route that reads messages:

1. **`MessageFilter`** (new, `timeline-core/src/message_filter.rs`; pure, no storage): the four
   filters above as types, parsed once from the request (`FlagFilter` and `FlagView` enums;
   `SearchText`, trimmed, lowercased, 1–200 characters; `TimeSpan`, never ending before it starts).
   Two questions it answers:
   - `admits_session(&session)`: can this session hold a match? No if it is another conversation,
     if its start and end miss the span, or if its stored count for the chosen flag under the chosen
     view is zero (§6); for "Flagged", if all three of its flag counts under the view are zero. Search can't rule a session out: only an index of words could (C17).
   - `admits_message(&message)`: does this message match? Every filter again, message by message,
     since a session with one flagged message holds unflagged ones too.
2. **`FlagView`'s rules** (new, `timeline-core/src/flag_view.rs`): whether a flag is in effect under
   a view, and whether a message counts towards a rate. These are today's `effectiveFlag` and
   `countsTowardRates` ([flags.js:12](../../frontend/core/flags.js#L12),
   [line 76](../../frontend/core/flags.js#L76)), moved to the server unchanged. The session counts
   (§6), the filter and the analyses (§5c) all use them, so "flagged" means the same everywhere.
3. **`find_messages(store, user, filter)`** (new, `timeline-api/src/message_query.rs`): lists the
   user's sessions (one query), keeps those `admits_session` accepts, reads each one's messages with
   one range query (the time is in the key, §3; the session's start and end bound it), and keeps
   those `admits_message` accepts. With no filter it reads every message.

**Who uses it:**

| Route | Filter it builds |
|---|---|
| `GET /messages` (Review) | the page's filters, as sent; then sorted (by time, or for a Calendar day by conversation name then time, as today) and cut to the requested page of 50. With replies on, each kept message's reply is read by its key |
| `GET /analyses/trend`, `GET /analyses/time-of-day` (§5c) | the view only |
| `POST /detect` (the scan) | one conversation at a time, view irrelevant |
| `GET /export` (the annotated download) | none: every message |
| Flag saves, recounting a session (§6) | that session's span and conversation |

No route reads message rows any other way, so a fix to the reading (paging, the key range) is made
once.

**What a search costs.** A search with no other filter reads every message of yours: on
`real-flags.json`, 2,255 rows, about 1 MB (0.4 MB of text plus each row's other attributes; estimated), one or two
DynamoDB calls, about 128 read units per search, so about 3 cents per thousand searches at
on-demand prices ($0.25 per million read units, from memory, not checked). One conversation, one day, or one flag reads only the
sessions that can match. It grows with how much you have written; C17 says when to add an index.

## 5c. Analytics: three in the page from sessions, two on the server, saved

Every flag rate is "messages with any of the three flags" over the counted messages (`isFlagged`
and `countsTowardRates`, [flags.js:76-85](../../frontend/core/flags.js#L76-L85)). The session rows
store both numbers for each view (§6), so the three analyses built from sessions run in the page
from the sessions it loads when the timeline opens, with no request (the user, 2026-10-06). The two
built from each message's own time are computed by the server and saved (the user, 2026-10-06);
the page draws every chart. As read in [analyses.js](../../frontend/core/analyses.js):

| Analysis | Needs | Where |
|---|---|---|
| Friction ranking, by session | each session's "any of the three" over its counted messages | **the page**, from the session counts |
| Friction ranking, by conversation | the same, added up over a conversation's sessions: exact, since each of your messages belongs to exactly one session ([flags.js:35-60](../../frontend/core/flags.js#L35-L60)) | **the page**, from the session counts |
| Session length vs. flag rate | each session's length and rate | **the page**, from the sessions |
| Idle time before a session | gaps between a conversation's sessions, and each session's rate | **the page**, from the sessions |
| Flag rate over time (week or month) | each message's local date and flags; a session can cross the end of a week or month | **the server**: `find_messages`, view only |
| Time of day and day of week | each message's local hour and weekday and flags | **the server**: `find_messages`, view only |

The three in the page keep their code in [analyses.js](../../frontend/core/analyses.js), reading a
session's stored counts instead of its messages (`sessionRate`, line 34, and the by-conversation
loop, line 46, change; nothing else does). With only your flags shown, a session's counted messages
are its reviewed messages, as today.

**The time zone.** Weeks, months, hours and weekdays are local to the viewer, so the page sends its
time zone's name (`Intl.DateTimeFormat().resolvedOptions().timeZone`, for example
`America/New_York`), and the server converts with **chrono-tz** (MIT or Apache-2.0; to be confirmed
from its licence file when added; `chrono` itself is already used). The week rule (weeks starting
Sunday, numbered from 1 January) is ported exactly. (The three analyses in the page use the
page's own time zone, as now.)

**Saving the two server analyses.** A result is saved in an `ANALYSIS#…` row (§3) keyed by the analysis, its options, the
view and the time zone, together with the user's data version at the time. The data version is
raised by every upload, flag save and scan. A request whose saved row has the current version gets
it back at once; otherwise the server computes it, saves it and returns it. So the first visit to
either after a change computes, and later ones don't.

**What the page gets from the server.** Numbers and identifiers (conversation ids, session start and end), not
text: labels such as "Conversation — Tuesday 4 August" are formatted in the page as now, since day
headings depend on the viewer's language settings. The page keeps `pearsonR`, the chart drawing
([charts.js](../../frontend/ui/render/charts.js)) and the click-through to Review.

## 6. Session flag counts

Each session row stores fourteen numbers, so that the Calendar, the Conversations tab, the flag
filter (§5b) and three of the analyses (§5c) never read messages:

- **your messages** in the session;
- **your reviewed messages** (with a value of yours for any flag; the same set as "overridden",
  §5b), which is a rate's denominator when only your flags are shown;
- for each view that shows anything (automatic only, yours only, both with yours winning), the
  messages with **critical**, **angry**, **ALL-CAPS**, and **any of the three**: twelve numbers.

With neither switch on, every count is zero and there is no rate, as today.

"Any of the three" is never shown as a count (the Calendar and the Conversations tab show each
flag's count separately: [conversations.js:81-89](../../frontend/ui/views/conversations.js#L81-L89),
[calendar.js:50](../../frontend/ui/views/calendar.js#L50)). It is stored because it is the top of
every flag rate, and it can't be added up from the three counts, since one message can carry two
flags. Storing it lets three analyses run in the page (§5c); the user briefly chose eleven numbers
without it, then restored it on 2026-10-06 once this was shown.

They change whenever a flag does:

- **A flag save** recounts that message's session in the same request: it reads the session's
  message rows through `find_messages` (a session has at most 112 messages here) and rewrites the
  session's counts.
- **The scan** recounts each session it finishes.

## 7. Processing an upload

After the file is parsed (once, as now), processing:

1. cuts each new or extended conversation into sessions with `build_blocks`;
2. extracts the files of §4 and stores them;
3. writes the rows of the new messages, then each new or changed session's row;
4. writes the conversation's record last, with its new version;
5. raises the user's data version (§5c);
6. deletes the uploaded file. A failed attempt leaves it, so a retry still has it.

**Writing faster.** About 4,800 rows here, in DynamoDB batches of 25: 192 batches. Sent one after
another at 10–30 ms each, that alone is 2–6 s. Processing sends up to 16 batches at a time, so the
writing should take well under a second; batches DynamoDB returns unfinished are resent, with a
pause that grows each time, up to 5 tries, after which the attempt fails with a distinct error
naming how many rows were left (and S3's own retry of the processing runs it again). How many at a
time DynamoDB accepts for one user's rows is C8.

**Timing each step.** The `processing_run` log line
([s3_trigger.rs:21](../../backend/timeline-api/src/s3_trigger.rs#L21)) records only the total today,
so how the measured 6 s on AWS divides between reading, parsing and writing is not known. It gains
one duration per step: reading the file, decompressing, parsing, sessions, files, rows, record.

**More memory, as an experiment.** The processing function has 512 MB
([template.yaml:417](../../infra/template.yaml#L417)). AWS gives a function processing power in
proportion to its memory (from memory, not checked: one full processor at about 1,769 MB), which
would explain processing and scan requests running about 5 times slower on AWS than on this machine
(inferred). Step 7 measures processing at 512 MB and at 1,769 MB, with the step durations, and the
cheaper setting per upload is kept.

On AWS each file of a batch is processed at the same time as the others. If two hold the same
conversation, both would read its record, add messages and write it back, the second write silently
replacing the first: a race that exists in the code built on 2026-10-05 too (C1). Processing
therefore writes the record only if its version is still the one it read (a DynamoDB conditional
write), and on a conflict re-reads and redoes that conversation. Session rows are keyed by
conversation and position and message rows by conversation, time and id, so a redo overwrites
rather than duplicates.

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

`POST /detect` reads your message rows instead of the file, and works within the request time
limit of §8c (about 9 seconds) before answering with how many sessions it has done of how many in
total, and where to carry on (the user's suggestion). The page asks again until all are done, so
its bar shows a real percentage. Each request stays well inside the deployment's 30-second limit
([template.yaml:148](../../infra/template.yaml#L148)). Tests start the server with a shorter
limit, so a small scan still takes several requests.

## 8b. The progress bar moves at least every 10 seconds while you wait

The user's rule (2026-10-06, clarified the same day): while the user waits, **the progress bar
moves, on real progress, at least every ten seconds**. A clock alone doesn't satisfy it. The
published guidance agrees on the threshold and says nothing finer: ten seconds is about how long
attention stays on a task, and past it users want a percent-done indicator, or they assume the
system has stopped (Nielsen's response-time limits); a bar that moves steadily is preferred to one
that stalls, and a stall with an explanation is better than a silent one (Harrison and others'
progress-bar studies). Waits that always end within ten seconds meet the rule without a bar.

**Estimates are not enough** (the user, 2026-10-06): a wait expected to take two seconds can take
thirty on a slow computer, on a slow connection, or with ten times the data. So the rule is met by
construction, not by estimate: **no wait contains a step whose length grows with the data, the
computer or the connection.** Every wait is a loop with a **time limit** (the user's approach,
first asked for the scan, 2026-10-06): it does as much as fits in its time, then stops, reports
how far it has got, and carries on. Chunks of a fixed size are not used to decide when to report: a
fixed number of items takes an unknown time on an unknown computer. Between checks of the clock
the loop does one *step*, the smallest piece of work there is (one message, one entry, one piece
the browser hands over), which takes milliseconds whatever the data:

| Kind of work | Time limit | Step between checks | Progress shown |
|---|---|---|---|
| A transfer (sending, receiving) | none needed: the browser reports as bytes move | | bytes of the stated size; every answer this server sends states its size |
| Preparing the file, in a Web Worker (§7b) | reports every **0.5 s** | one piece of the file stream; one message slimmed; one piece compressed | bytes read and parsed, conversations slimmed, bytes compressed, each of its total |
| Drawing the timeline in the page | hands the screen back every **50 ms**, reporting each time | one session drawn | sessions drawn of the total |
| A server request (the timeline's records and sessions, Review's messages, a search, the two server analyses, the scan, the annotated download) | answers when its work is done, or at the **request time limit** (§8c: about 9 s of work, so that with the round trip the bar moves within 10 s), with what it has and a **cursor** (where to carry on); the page asks again | one entry read, matched, scanned or written | sessions (or conversations) done of the total |
| Processing an upload | writes its progress row every **1 s**; the page reads it every second (exists) | one message parsed and written | bytes of the file read, conversations written |

**Totals known in advance.** A bar needs a total. The user's record (which already holds the data
version, §5c) also holds the counts of conversations, sessions, your messages and all messages,
kept current by processing. A request that walks sessions reports "done of total" against them.

**What this changes, wait by wait:**

- **Opening the timeline.** `GET /conversations` and `GET /sessions` each answer within the request
  time limit with the total and a cursor; the bar is conversations, then sessions, received of their
  totals, and within a part bytes of the part. Drawing the Calendar and the Conversations tab then
  runs in 50 ms turns, with the bar showing sessions drawn.
- **Review, any filter, including search** (closes C17's stall; its cost stays). The shared
  `find_messages` (§5b) gains the request time limit and the cursor: it reads the admitted sessions in
  key order, stops at the budget, and returns what it found with "sessions searched of sessions to
  search". The page shows the first page of 50 as soon as it has 50 (or the search ends), with the
  bar still moving below it until the search finishes; the page count is final only then, and says
  "at least N pages" until it is.
- **The two server analyses.** Computed in the same steps: each request adds the next sessions'
  messages to a partial result kept in the analysis row (§5c) with its cursor, and answers with
  sessions done of the total. Once finished, the row is the saved result, as before.
- **The scan.** It checks the clock after every message, not after every conversation, so one very
  long conversation can't make a request run past its time limit; the cursor can stop inside a
  session.
- **The annotated download.** Built in parts the same way into the file store, then received with
  its size stated.
- **Processing.** The file is parsed as a stream (serde's streaming sequence access over the
  decompressing reader), not in one call, so a large file is never one long step; the progress row
  is written every second with bytes read of the file's size and conversations written.
- **Preparing the file in the browser.** As before: a Web Worker, the file read as a stream and
  parsed with **@streamparser/json** (MIT; to be confirmed from its licence file when vendored),
  reporting every half second.

Why the first version of this section used fixed chunks (200 sessions per frame, one conversation
or one session per check): it copied the page's existing pattern (`computeWithProgress` in
[analytics.js](../../frontend/ui/views/analytics.js), 400 items per chunk) instead of the time
limit the user had already chosen. Corrected the same day at the user's question.

**What it costs.** Only work longer than the time limit is split: a search needing 6 seconds of
server work is still one request (the user, 2026-10-06: a 2-second limit would have split it into
three for no reason); one needing 25 seconds is three, each with its own sign-in check and round
trip (about 50–100 ms on AWS, from memory, not checked). Analysis rows hold partial results while being computed. The page's
code for Review gains a loop it didn't have.

**The one step left that can't be split:** a single entry. The largest message here is 18 KB, and
DynamoDB refuses rows over 400 KB, so a row can't grow past that; reading or scanning one takes
milliseconds. A single message too large to store is refused at processing with a message naming
it.

**What happens today** (read in the code, 2026-10-06):

- **Signing in, reading the file, processing:** a striped, animated bar at full width
  (`setLoadProgressIndeterminate`) that measures nothing. Processing took about 6 s on AWS.
- **Sending:** a real bar, bytes sent ([status-indicators.js:170](../../frontend/ui/widgets/status-indicators.js#L170)).
- **Opening the timeline:** the striped full bar for the whole `GET /export` (12.3 s measured on
  AWS), so nothing moves for over ten seconds; then bytes received, as a real bar when the size is
  stated, otherwise a full bar with the bytes in the label.
- **The scan:** the bar's width moves after each request of 5 conversations (about every 3 s on
  AWS). But the striped animation set for processing is never removed (`showScanProgress` changes
  only the width, [status-indicators.js:185](../../frontend/ui/widgets/status-indicators.js#L185)),
  so the measured bar is drawn as if it measured nothing, and until the first answer it stays at
  full width (inferred from the code; not seen on screen). Fixed by this plan: every measured bar
  clears the striped state.

**The clock is not progress, and is labelled so.** Real progress is known only at certain moments:
as bytes are sent or received, and when a server request answers (the scan's done-of-total). In
between, inside one request or one step of preparing the file, nothing can be measured: the page
can't see how far the server has got. What changes every second there is a clock the page keeps
itself, worded as time spent, not as work done: "Scanning your messages — 41 of 117 conversations
(35%) · 12 s so far". The bar moves only on real progress. One shared piece of the status line
(`status-indicators.js`) does this for every wait, as the processing wait already does; the
activity record notes the label once, not each tick (as now, `setLoadProgressLabel`).

**The clock must be able to tick.** A browser page runs its own code on one thread: while it is
busy, it redraws nothing, clock included. Preparing the file reads the whole 63.5 MB export in one
step (`JSON.parse`, 0.3–0.7 s in Node here, possibly much longer in a browser on a slower computer,
C11), and nothing can interrupt that step to report how far it got. So preparing the file runs in a
Web Worker (a second thread the browser provides, built in; no library), which sends back its
progress as it goes; the page's thread stays free, and the bar and clock keep moving. Measured in
step 7: the longest gap between two moves of the bar, in each wait, on the user's computer and on
AWS; any gap over ten seconds is a failure of this plan.

## 8c. Requests in parts: the time limit, and how the page carries on

**The time limit.** The user chose 10 seconds (2026-10-06). The bar moves when an answer arrives,
so the limit is on the server's work, set so that work plus the round trip stays within 10 seconds:
the server starts no new step after **9 seconds** of work (a step takes milliseconds, §8b), and
answers. On AWS, API Gateway added about 0.1 s to our code's time (3.0–4.3 s against 2.9–4.2 s,
measured 2026-10-05); the trip between the user's browser and AWS was not measured, and the
one-second margin assumes it is well under a second. Step 7 measures the gap between two moves of
the bar end to end. A
request that finishes sooner answers sooner; nothing is split before the limit. Tests run the
server with a shorter limit (an environment setting) so that small test data still makes several
requests.

**The server keeps no state between requests**, except where noted below. Each answer carries a
**cursor**: an opaque text the page sends back unchanged with the same request to carry on. The
server parses it (it crosses a trust boundary: the page could send anything) into a typed
`Cursor` naming where the previous request stopped: the session (conversation and start time) and
the last entry key done within it, or the last DynamoDB key read. A cursor that doesn't parse is a
400 naming what was wrong; a cursor can only point into the signed-in user's own rows, since the
user comes from the sign-in, never from the cursor. Each answer also carries the user's **data
version** (§5c); if it changes between two parts, data changed underneath (an upload finished, a
flag was saved elsewhere), so the page starts that request over from the beginning and says so
("Your data changed; starting again").

| Request | What the page sends back | What the page keeps between parts |
|---|---|---|
| `GET /conversations`, `GET /sessions` | the cursor | the records and sessions received so far (it keeps these anyway) |
| `GET /messages` (Review: any filter, search) | the same filters and the cursor | only what the page needs to show and to page: the rows of the page on screen, how many matches so far, and **for each page of 50 its starting cursor**, so "page 7" asks the server to carry on from page 7's cursor and collect 50, without the page ever holding every match. Until the search finishes, the page count reads "at least N pages" |
| `POST /detect` (the scan) | the cursor | nothing: flags are written as the scan goes; only the bar's numbers |
| `GET /analyses/{name}` (the two server analyses) | the same options; no cursor | nothing. **This one keeps state on the server**: the partial result and its cursor are kept in the analysis's own row (§5c), keyed by the analysis, its options, the view and the time zone, so any request with the same options carries on from it; a finished row is the saved result |
| `GET /export` (the annotated download) | the cursor | the parts received so far, joined into the file as a `Blob` in the page and saved when the last part arrives. Each part is also capped at 4 MB, under Lambda's 6 MB limit on an answer |

**Stop and leaving the page.** The page stops asking; nothing on the server is left running (each
request has already answered). A half-computed analysis row is carried on by the next request
with the same options.

**Every analysis can stop part-way and carry on** (the user, 2026-10-06):

- **The two on the server** (rate over time, time of day): their partial result and cursor are in
  the analysis row, as above. Leaving Analytics, or the page, stops them; the next request with the
  same options carries on; a changed data version starts them over.
- **The three in the page** (friction ranking, session length, idle time): they run in the page's
  50 ms turns (§8b) over the sessions it holds. Each keeps its partial result and where it stopped
  in the page's memory, keyed like the server's (analysis, options, view, and the data version of
  the sessions it holds). Leaving the tab stops it; coming back carries on from where it stopped. A
  reload loses the partial result, and the analysis starts over from the sessions, which the page
  loads again anyway; nothing is fetched for it.

**How the time limit is made testable.** A test can't rely on a computer being slow enough to hit a
time limit. So every loop of §8b takes its limit as a `WorkBudget` (Rust) or `workBudget` (page): a
value that answers "may I do another step?". The real one answers by the clock (9 s on the server;
50 ms or 0.5 s in the page); the tests' one answers by counting steps, so a test can stop a request
after exactly one, two or N steps. The loop's code is the same either way; only the budget handed to
it differs. The server reads which to use from a setting that only the local test server sets (as
the scan's shorter limit was going to be).

**Tests that resumption works, in every case.** For each request that answers in parts, and each
page analysis:

| What is tested | How |
|---|---|
| Parts add up to the whole | with a budget of 1, 2 and 3 steps, the joined parts equal the one-request answer: the same records, sessions, matches in the same order, flags written, analysis numbers, download file |
| A cursor stops anywhere | the stop falls in the middle of a session, at a session's last entry, at a conversation's end, and at the very end (the last answer carries no cursor) |
| Starting from a given cursor | without running out of time: a request started from a cursor built for a known point in the test data (mid-session, a session's start, a conversation's end) returns exactly the answer worked out in advance for the rest of the data (the user's suggestion, 2026-10-06) |
| Nothing is done twice or skipped | every entry is read, scanned or written exactly once across the parts (the scan's write count; the search's match ids) |
| A changed data version starts over | a flag saved between two parts; the next answer says so and the page restarts |
| A bad cursor is refused | text that isn't a cursor, a cursor of another request kind, a cursor naming a session that no longer exists: each a 400 naming why |
| Another user's data can't be reached | a cursor made from user A's request, sent by user B, reads only B's rows |
| Stop and carry on | an analysis stopped after one part and asked again later carries on from its saved row (server) or its kept partial result (page) and ends with the same numbers |
| In the browser | Review paging forward and back through more than 50 matches with a 1-step budget; a search whose result count grows across parts; the scan's bar reaching 100% over several parts |

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

**Why processing on AWS stays near 6 s, and what might change it.** The measured 6 s covers reading
the 63.5 MB file from storage, parsing it and writing about 120 records; the parts are not
measured separately (§7). After this plan it parses 9.3 MB, an eighth as much, but writes about
4,800 rows instead of 120. The "After" figure below assumes the parallel writes of §7 at 512 MB:
parsing about 0.5 s locally, so about 2.5 s on AWS at the measured ratio, plus under 1 s of writing
and the function's start. It is a guess until step 7 times each step; more memory may shorten it.

| What you wait for | Today, local | Today, AWS | After, local | After, AWS |
|---|---|---|---|---|
| **Preparing the file in the browser** (§7b) | none | none | 1.5 s | 1.5 s |
| **Sending the file** | 26 s (63.5 MB) | 26 s (measured) | 1 s (2.8 MB) | 1 s |
| **Processing** (from the file arriving to Describe) | 1 s (measured) | 6 s (measured) | 0.5 s | 3–4 s |
| **Scanning, if ticked** | 15 s (measured) | 80 s (measured) | 1 s | 5 s |
| **Opening the timeline** | 27 s | 37 s (measured, during an upload) | 0.1 s | 1 s |
| **A page of Review** (filters, search or page changed) | already loaded | already loaded | 0.1 s | 0.3–1 s |
| …a search with no other filter (reads every message) | already loaded | already loaded | 0.2 s | 1–2 s |
| …with Claude's replies on | already loaded | already loaded | 0.2 s | 0.5–1 s |
| **Friction, session length, idle time** (in the page, from sessions) | under 1 s, in the page | under 1 s, in the page | instant | instant |
| **Rate over time, time of day**, first time after a change | under 1 s, in the page | under 1 s, in the page | 0.3 s | 1–2 s |
| …saved | | | 0.05 s | 0.2 s |
| **The annotated download** | 27 s | 37 s | 3 s | 5 s |

So the wait from pressing Upload to Describe, with the scan ticked, goes from about 112 s on AWS
today to about 11 s. Review and Analytics, which today cost nothing once the whole export is in the
page, now cost a request each time; that is the trade for never holding every message in the page
(C13).

**Cost per upload of this export** (AWS's published us-east-1 prices, from memory, not checked):
DynamoDB writes of about 8,000 write units, roughly $0.005–0.01; about 50 S3 writes, negligible.

## 10. Order of work

**Already done** (2026-10-06, commit `3fde146`): core types, compiled but **untested and not yet
called by anything**:
[flag_values.rs](../../backend/timeline-core/src/flag_values.rs) (`FlagSet` and `FlagOverrides`
moved here from the flags port, `MessageFlags`, `FlagKind`),
[flag_view.rs](../../backend/timeline-core/src/flag_view.rs) (`FlagView` over the existing
`flags::matrix::effective_flag`),
[stored_message.rs](../../backend/timeline-core/src/stored_message.rs) (`EntryKey`, `Entry`,
`StoredMessage`, `BranchNote`, `Piece`, `Citation`, `FileRef`),
[stored_session.rs](../../backend/timeline-core/src/stored_session.rs) (`SessionCounts` with the
fourteen numbers, `cut_sessions`),
[message_filter.rs](../../backend/timeline-core/src/message_filter.rs) (`MessageFilter`,
`SearchText`, `TimeSpan`, `FlagFilter`), [branches.rs](../../backend/timeline-core/src/branches.rs)
(`prune_replaced_branches`), and `ChatMessage::parent()` in
[model.rs](../../backend/timeline-core/src/model.rs). They predate §4e (unknown times), §8b and §8c
(time limits, cursors), so they need: `MessageTime` and the sentinel; a placed-by-span session
(§4e) and `admits` taking the session for messages of unknown time; the cursor. The old flags
port's types are still in [ports/message_flags.rs](../../backend/timeline-core/src/ports/message_flags.rs)
too, until step 1 removes that port.

1. **Storage.** Session and message rows with flags on them, message keys with the time (the zero
   date for unknown, §4e) (core types and ports, in-memory and DynamoDB adapters), range reads of
   one session's messages that can start from a cursor, analysis rows (with partial results) and
   the user's record (data version; counts of conversations, sessions and messages, §8b), the files
   store with an object delete, reads in parts with a cursor, the conversation record's version and
   `untimed` count; the `MessageFlags` table removed from the template.
2. **Processing.** The upload read as a stream (gzip or plain), one conversation at a time;
   progress written to the progress row every second; sessions (cut by gaps, or placed by span,
   §4e), messages, notes and files written with the versioned write and its retry, up to 16 batches
   at a time; step durations in its log line; an upload already processed skipped; the upload
   deleted when done; the additions objects and `conversation_rebuild.rs` replaced.
3. **The time limit and cursors** (§8b, §8c). `WorkBudget` (clock, or counting steps for tests),
   the typed `Cursor`, the data version in every answer.
4. **Flags and their counts.** `FlagView`'s rules on the server; flag saves finding their message
   among the conversation's rows and recounting its session; the scan writing message rows; the
   fourteen session counts kept current; the data version raised.
5. **Finding messages** (§5b). `MessageFilter` and `find_messages` with the time limit and cursor,
   then the routes on them: `GET /conversations` and `GET /sessions` in parts, `GET /messages`,
   `GET /conversations/{id}/files`, `GET /files/…`; the annotated download in parts, rebuilt from
   rows.
6. **Analytics** (§5c). Rate over time and time of day on the server, with the time zone, partial
   results and saved results.
7. **The scan** (§8). Rows instead of the file; the time limit; a cursor that can stop inside a
   session.
8. **Slimming the upload** (§7b): in a Web Worker, with @streamparser/json, progress every half
   second; the server's decompression.
9. **Replaced branches** (§4d): pruning and notes in processing; important branches kept as their
   own conversations; a revived branch found by its parent; notes in sessions and Review.
10. **Files and citations in Review** (§4, §4c): file extraction in processing (replayed edits,
    widgets, attachments, the "may have been changed later" mark); file cards in place in Claude's
    replies, the Conversations tab's list of a conversation's files, numbered citation links;
    highlight.js vendored.
11. **The page.** The timeline from conversations and sessions, received and drawn in time-limited
    parts; Review a page at a time from `GET /messages`, carrying on with cursors, one cursor kept
    per page of 50; two analyses from `GET /analyses`, three from the sessions in 50 ms turns,
    each resumable; the same session flags on each day a session touches; the progress bar of §8b
    for every wait (the striped state cleared by every measured bar); `state.humanMessages`, the
    page's filtering and the two moved analyses removed; Describe's warning for conversations with
    messages of unknown time.
12. **Tests and measurements.** Every step tested as in the screen-flow plan, with 100% coverage
    through public interfaces; the replacement tests of §10b's matching table, each naming the test
    it replaces; the resumption tests of §8c, including runs from cursors whose results were worked
    out in advance; then §9 and §8b's longest gap between moves of the bar measured locally and,
    after a deployment, on AWS, with processing at 512 MB and 1,769 MB, and written up as an
    analysis.

The changes to existing tests are listed in §10b; the user approved them on 2026-10-06.

## 10b. Changes to existing tests (approved by the user, 2026-10-06)

Found by reading every test on 2026-10-06. Choices made first, to keep the list short (each also
avoids a behaviour change nobody asked for):

- **Plain uploads still accepted.** Processing decompresses a gzip-compressed upload and reads any
  other as plain JSON, so tests that store plain JSON keep working.
- **Flag handles unchanged.** `GET /messages` issues them; `GET /export` still does too. A save
  finds its message among the conversation's rows by id, so a save's body is unchanged.
- **`GET …/flags` kept**, read from the message rows.
- **The annotated download keeps `_claude_timeline_auto`** for scanned messages.
- **The `additions` field stays** on the conversation record (the uploads that added messages; the
  file list's "gained messages" reads it); only the `additions/…` objects go.
- **A later file still adds only messages outside the stored time range** (Q18).
- **An upload already processed is skipped**: since the original is deleted, a repeated S3 event
  would otherwise fail and overwrite "ready" with "failed".

**Backend (Rust).**

| Test | Change | Why |
|---|---|---|
| `timeline-core/tests/model.rs` `a missing created_at is caught the same` (line 129) | rewritten: a missing `created_at` is read as unknown (§4e); a malformed one is still refused (its own test, unchanged) | the user, 2026-10-06 |
| `timeline-storage/tests/memory_message_flags.rs`, `dynamo_message_flags.rs`, `support/message_flags_contract.rs` (all) | **remove** | the `MessageFlags` table and its port go; equivalent contracts are written for flags on message rows |
| `timeline-storage/tests/contract_memory.rs` | drop the flags contract module | same |
| `timeline-storage/tests/support/conversation_summary_contract.rs`, `memory_conversations.rs`, `dynamo_conversations_table.rs` | records gain `version`; `put_again_replaces_the_earlier_summary` becomes "a write with the version read replaces it; a stale one is refused" | versioned writes (§7, C1) |
| `timeline-core/tests/raw_object_key.rs` `added_messages_are_kept_under_their_user_conversation_and_upload` | **remove** | `addition_object_key` goes |
| `timeline-api/tests/processing.rs` | harness without the flags store; `processing_an_upload_writes_no_automatic_flags`, `reviews_embedded_in_an_upload_are_stored_as_yours`, `an_empty_review_is_not_stored`, `detection_after_upload_keeps_the_imported_review`, `an_unreadable_review_fails_the_upload_and_stores_nothing` read flags from message rows | flags move onto rows |
| `timeline-api/tests/processing_merge.rs` | harness; `processing_the_same_file_again_adds_nothing_twice` keeps its assertions | |
| `timeline-api/tests/processing_errors.rs` | `a_failed_review_save_names_the_review_its_number_and_the_total` becomes "a failed message-row write names how many rows were left"; the failing summary store implements the versioned write | reviews are written with the rows, in batches |
| `timeline-api/tests/detect.rs` | `paging_covers_every_conversation_exactly_once` rewritten for the time budget (the next conversation to start from, done of total); other tests' response fields renamed | §8 |
| `timeline-api/tests/conversation_metadata.rs` | `the_scan_reads_messages_a_later_file_added` and `damaged_stored_data_is_a_server_error` rewritten: the export and scan read rows, so damage is a damaged row, not a damaged stored file | `conversation_rebuild.rs` goes |
| `timeline-api/tests/request_log.rs` | `detection_uploads_and_exports_log_their_facts`: the scan's logged facts change | §8 |
| `timeline-api/tests/run_log_lines.rs` | `a_ready_upload_logs_what_was_read_and_stored_on_its_own_channel`: the logged facts gain each step's duration | §7 |
| `timeline-api/tests/s3_trigger.rs` `processing_an_event_twice_stores_the_same_data_as_once` | keeps its assertions; passes because the second event is skipped | |
| `timeline-api/tests/storage_settings.rs`, `aws_state.rs`, `support/aws_world.rs` | the message-flags table setting goes (six settings become five) | |
| `timeline-api/tests/aws_state.rs` `export_and_a_flag_save_on_one_instance_are_visible_on_another` | stores message rows, not only a raw file | the export reads rows |
| Every test router and store set (`app.rs`, `export.rs`, `detect.rs`, `flag_saves.rs`, `conversation_metadata.rs`, `dev_routes.rs`, `lambda_router.rs`, `upload_status.rs`, `request_log.rs`, `s3_*`, `deliberate_failure.rs`, `run_log_lines.rs`) | setup only: the flags store is replaced by the message store; `FaultyObjectStore` gains `delete` | |

**Page unit tests.**

| Test | Change | Why |
|---|---|---|
| `analyses.test.js` | the fixtures build sessions with counts instead of messages; the trend and time-of-day tests (L55, L70, and those parts of L111) **move to Rust** | §5c |
| `flags.test.js` L49 (`attachFlags`), L79 (`countsTowardRates`) | **remove**; the rules are tested in Rust | sessions come counted from the server |
| `blocks.test.js` L13, L27; `blocks-span.test.js` (all) | **remove**; session cutting and placing by start and end are tested in Rust (§4e) | |
| `export-format.test.js` | all but L69 **removed** or rewritten as tests of slimming (L32, L69 kept on the slimming function) | the page no longer reads an export into its state |
| `annotated-export.test.js` L13, `api-client-unit.test.js` L57, L67, L77 | rewritten: the download comes from `GET /export`; handles come from `GET /messages` | |
| `page-messages.test.js` L34 | wording list follows the changed messages ("Loaded N of your confirmed flags" goes; "Preparing the file" comes) | |
| `fixtures.js` | builds sessions with counts; drops the parsed-export fields | |

**Browser tests.**

| Test | Change | Why |
|---|---|---|
| `synthetic-export.js` | chains each message's parent to the one before; options to make branches, citations, attachments and file tool calls | today every message is a first message, so pruning would treat all but one as replaced |
| `upload-flow.spec.js` L285 (a file over 2 MB) | sends the large file without slimming, through the plain-upload path, so the size limit is still tested | |
| `upload-flow.spec.js` L327 | checks the row's reviewed state after reload, not the "Loaded N flags" line | |
| `upload-flow.spec.js` L348 | **rewritten** for what opening the timeline now fetches: conversation records and sessions, answered without a stated size, show what has arrived (the user, 2026-10-06) | |
| `activity.spec.js` L146 | expects the scan's new request shape | |
| `cognito-login.spec.js` L308, `views.spec.js` L399, L669, L683, L802 | hold or fail `GET /sessions` instead of `GET /export` | |
| `views.spec.js` L173 | tests search, filters and paging through `GET /messages`, with more than 50 messages | |
| `views.spec.js` L235, L699 | check the rebuilt download (text, flags, file names) | |
| `screen-flow.spec.js` L489 | the backend is started with a shorter scan budget for tests, so the scan takes several requests | |
| `views.spec.js` `waitForAnalysis`, L150, L647 | wait for the analysis's request or its drawn chart | two analyses now come from the server |
| `views.spec.js` L842 | extended: the same flags on both days of a session crossing midnight | |

### Every removed test and the backend test that replaces it

The user (2026-10-06) asked for each removed test to be matched one-to-one with a new backend test.
Each new test names the one it replaces in its doc comment.

| Removed | New backend test |
|---|---|
| `memory_message_flags.rs` `get_before_any_write_is_none` | message-row contract: a stored message has no automatic flags and no review |
| … `auto_write_is_visible_via_read_with_no_user_overrides_set` | contract: an automatic write is read back, with no review |
| … `user_write_is_visible_via_read_without_disturbing_auto` | contract: a review write is read back, automatic flags unchanged |
| … `a_partial_user_update_only_touches_the_flags_it_names` | contract: a partial review changes only the flags it names (in-memory) |
| … `list_for_conversation_returns_every_message_in_it` | contract: reading a conversation returns every entry in it, in time order |
| … `list_for_conversation_does_not_include_a_different_conversations_messages` | contract: reading a conversation leaves out other conversations' entries |
| … `flags_are_isolated_per_user` | contract: one user's rows are invisible to another (in-memory) |
| `dynamo_message_flags.rs` `a_row_whose_sort_key_is_not_a_message_id_is_a_backend_error_when_listed` | DynamoDB: a message row whose key isn't a conversation, time and id is a backend error |
| … `every_method_reports_a_missing_table_as_a_backend_error` | DynamoDB: every message-store method reports a missing table as a backend error |
| … six `…_stored_as_…_is_an_error` tests (`auto_caps`, `auto_critical`, `auto_angry`, `user_caps`, `user_critical`, `user_angry`) | DynamoDB: the same six attributes, of the wrong type on a message row, are each an error naming the attribute |
| `message_flags_contract.rs` `get_before_any_write_is_none` | contract: reading a message that was never written is `None` |
| … `an_auto_write_sets_no_user_override` | contract: an automatic write sets no review |
| … `a_second_auto_write_replaces_the_first_and_keeps_user_overrides` | contract: a second scan replaces the first and keeps the review |
| … `a_user_write_does_not_disturb_auto_flags` | contract: a review write leaves the automatic flags alone |
| … `set_user_flags_returns_the_record_as_stored` | contract: a review write returns the flags as stored |
| … `a_user_write_on_a_message_with_no_record_creates_one_with_no_auto_flags` | contract: a review write on a message with no row is not found and creates nothing (rows exist for every message now, so a write without one names a message that isn't stored) |
| … `a_partial_user_update_only_touches_the_flags_it_names` | contract: a partial review changes only the flags it names (both stores) |
| … `an_empty_user_update_on_an_existing_record_changes_nothing` | contract: an empty review changes nothing |
| … `an_empty_user_update_on_a_message_with_no_record_is_not_found_and_creates_nothing` | contract: an empty review on a message with no row is not found and creates nothing |
| … `list_for_conversation_returns_every_message_in_it` | contract: reading a session's span returns exactly its entries |
| … `list_for_conversation_excludes_other_conversations` | contract: reading a span leaves out other conversations at the same times |
| … `flags_are_isolated_per_user` | contract: one user's rows are invisible to another (both stores) |
| `raw_object_key.rs` `added_messages_are_kept_under_their_user_conversation_and_upload` | a stored file is kept under its user, conversation, message and number, and no file name becomes part of the key |
| `flags.test.js` L49 `attachFlags files each message under its session, or the nearest one` | each of your messages is counted in exactly the session that holds it; in a conversation placed by its start and end (§4e), timed messages outside that span are counted in its one session (the "nearest one" case: the page's fallback existed for exactly these messages) |
| `flags.test.js` L79 `only messages with a value under the switches count toward rates` | `FlagView::counts_toward_rates` under all four views |
| `blocks.test.js` L13 `a gap of 15 minutes or more starts a new session` | `cut_sessions`: a pause of 15 minutes starts a session, 14:59 does not |
| `blocks.test.js` L27 `a session continues across midnight, and never spans two conversations` | `cut_sessions`: one session across midnight; two conversations at the same times give separate sessions |
| `blocks-span.test.js` L20 `a conversation with no message times is one session from its start to its end` | §4e: one session from the record's start to end |
| … L29 `editing its start and end moves it` | §4e: editing the span rewrites the session |
| … L35 `some timed messages and some not` | §4e: placed by start and end, not by the timed messages |
| … L44 `timed conversations, empty ones and ones without a record are placed as before` | §4e: timed conversations are cut by gaps; an empty one has no session. ("Without a record" can't be matched: on the server every conversation has a record) |
| `export-format.test.js` L6 `FORMAT_VERSION marks files this page saved` | the annotated download is marked with its format version (now written by the server) |
| … L10 `extractMessageText joins text pieces and skips other kinds` | a stored message's text joins its text pieces and skips file marks |
| … L16 `a wrapped export is read as already processed` | an annotated download uploaded again is processed with its reviews |
| … L41 `a message without a timestamp is counted but not placed on the timeline` | §4e: counted in the conversation, found by the conversation filter, not by a Calendar day |
| … L48 `embedded automatic and confirmed flags are read from their separate fields` | processing takes `_claude_timeline_user` as your review and never takes `_claude_timeline_auto` as one |
| … L74 `a review field with no flag stated is not counted as a review` | processing: a review field with no flag stated leaves the message unreviewed |
| … L84 `each conversation's id and its count of untimed messages come beside the list` | the conversation record carries its id and `untimed` count (§4e) |
| `analyses.test.js` L55 `the trend buckets by week by default, or by month` | trend analysis: weeks by default, months on request, in the viewer's time zone |
| … L70 `time of day counts by hour and weekday` | time-of-day analysis: counts by local hour and weekday |
| … L111, its trend and time-of-day parts | both server analyses, with only your flags shown, count reviewed messages only |
| `upload-flow.spec.js` L348 `a timeline download without a stated size reports what has arrived` | replaced by a browser test, not a backend one (the user, 2026-10-06): loading the timeline's conversation records and sessions, answered without a stated size, shows in the loading modal what has arrived. The 64 MB timeline download it tested no longer happens; the records and sessions are what opening the timeline now fetches |

## 11. Questions for the user

**Answered on 2026-10-06:** Q1 (withdrawn: flags move onto the message rows, §3); Q2 (the upload is
deleted once processed, §7); Q4 (the path to each conversation's latest message is kept, §4d); Q5
(a replaced branch is kept as its own conversation at 100 words or more once messages repeated on
the kept path are removed, §4d); Q3 (SVGs drawn; web pages and code shown without running anything, with
a download link, §4). The session files of the previous draft are dropped (the user).

**Also decided on 2026-10-06:** messages load only as a view shows them (§5); Review's filters
and search run on the server, narrowed by sessions first, through code shared by every route that
reads messages (§5b); the two analyses that need each message's time are computed on the server and
saved, the other three in the page from the session counts, and every chart drawn in the page (§5c).

**Still open:** none.

## Self-critique log

### C1 [OPEN]: Two files of a batch processed at once can lose each other's messages
Present in the code built on 2026-10-05 as well. **Mitigation in plan:** versioned conditional
writes with a retry ([§7 (line 341)](2026-10-06-load-only-what-the-page-shows.md#L341)). **Open:** until
this plan is built, a batch whose files share conversations can lose added messages on AWS. Trigger:
this plan's step 2.

### C2 [RESOLVED]: Your messages in one file would race too
**Resolution:** one database row per message ([§3 (line 34)](2026-10-06-load-only-what-the-page-shows.md#L34)).

### C3 [OPEN]: The estimates are not measurements
§9's figures, other than those marked measured, are estimates on stated assumptions. **Open:** step 7
measures them.

### C4 [RESOLVED]: The first estimates table mixed kinds of numbers
**Resolution:** one table, the same four columns, absolute times ([§9 (line 425)](2026-10-06-load-only-what-the-page-shows.md#L425)).

### C5 [RESOLVED]: Files and rows both holding messages
**Resolution:** rows hold every message's text; the session files are dropped, and tool calls,
tool results and thinking are not kept (the user, 2026-10-06). Only files of §4 are stored
([§4 (line 63)](2026-10-06-load-only-what-the-page-shows.md#L63)).

### C6 [RESOLVED]: The Calendar needed messages only to split a session's flags by day
**Resolution:** a session's flags show on every day it touches; the counts are stored with the
session ([§6 (line 314)](2026-10-06-load-only-what-the-page-shows.md#L314)).

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
step, and derives the slow-down from them ([§9 (line 425)](2026-10-06-load-only-what-the-page-shows.md#L425)).

### C11 [OPEN]: Slimming is measured in Node, not in a browser
Parsing the 63.5 MB file took 0.3–0.7 s in Node on this machine; a browser on a slower computer may
take several seconds, and holds the whole file in memory meanwhile, as it already does today.
**Open:** step 7 measures it in the user's browser. Trigger: if preparing takes over 10 s, slim on
the server instead (sending the full file again).

### C12 [RESOLVED]: A later export can continue a branch that was pruned
The path is decided per upload, so a later export can show that the branch kept earlier was itself
replaced. **Resolution:** the user decided (2026-10-06) that a replaced branch worth keeping becomes a
conversation of its own; processing finds a revived branch by each new message's parent among the
stored rows ([§4d (line 114)](2026-10-06-load-only-what-the-page-shows.md#L114)). What is worth
keeping was decided with Q5.

### C13 [RESOLVED]: The first draft loaded every message into the page
It loaded all your messages in the background and all of Claude's replies at once, although loading
per conversation had been discussed, and it said neither that it departed from that nor why. Holding
everything costs memory without limit as history grows, start-up time and data, stale copies, and
exposure of every message to whatever else runs in the tab. **Resolution:** views ask for what they
draw; Review a page at a time, two analyses as saved numbers, three from the sessions
([§5 (line 182)](2026-10-06-load-only-what-the-page-shows.md#L182)).

### C14 [RESOLVED]: Nine session counts could not serve the "Flagged" filter or a rate
"Flagged" is any of three flags, which can't be added up from per-flag counts, and a rate with only
your flags shown is over your reviewed messages, which weren't counted. **Resolution:** the
messages, your reviewed messages and "any of the three" are counted per view: fourteen numbers.
The user briefly dropped "any of the three" (2026-10-06), as it is never shown as a count, and
restored it once it was shown to let three analyses run in the page without a request ([§6 (line 314)](2026-10-06-load-only-what-the-page-shows.md#L314)).

### C15 [RESOLVED]: Processing on AWS was put at 6 s after the change without a reason
**Resolution:** the estimate is derived (parsing at the measured AWS ratio, parallel writes), the
log line gains step durations, and step 7 tries more memory
([§7 (line 341)](2026-10-06-load-only-what-the-page-shows.md#L341)).

### C16 [RESOLVED]: Analyses on the server don't know the viewer's day, week or hour
**Resolution:** the page sends its time zone; results are saved per time zone
([§5c (line 273)](2026-10-06-load-only-what-the-page-shows.md#L273)).

### C17 [OPEN]: A search with no other filter reads every message
**Mitigation in plan:** every other filter narrows sessions first (§5b), and a long search answers
in parts with the bar moving (§8b, §8c), so it is slow but never silent. **Open:** an index of words
(rows per word written in processing, or the `tantivy` search library, MIT) would avoid the full
read, but matches whole words where today's search matches letters inside words, so it changes what
search finds. Trigger: a search with no other filter taking more than one request (9 s of work) on
AWS in step 12, or a user's messages passing 10 MB.

### C18 [OPEN]: Lowercasing may differ between the page and the server
Today's search lowercases in JavaScript; the server would use Rust's `to_lowercase`. They agree for
English letters; a few letters in other alphabets lowercase differently (inferred from the two
languages' documentation, not tested). **Mitigation in plan:** tests with accented and non-Latin
letters in step 4. **Open:** if those tests show a difference that matters, the page sends the text
already lowercased.
