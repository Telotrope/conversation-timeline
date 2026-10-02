# Record what the user did and what reached AWS

## Context

During the 2026-10-02 deployment checks
([analysis](../analysis/2026-10-02-deployment-checks-status.md)), the logs could not say what the
user did. The API function logs only Lambda's own start/end/`REPORT` lines, with no route, method
or result; API Gateway's request logging is off; the page records nothing. So Claude could not
tell whether detection ran (D5), which request was a flag save (D6), or why processing ran twice,
and had to ask the user or guess from timing.

The user asked (2026-10-02) for instrumentation sufficient to answer the deployment checks from
records alone, and more: every click in the interface over a period such as 24 hours, and the
actual calls made into AWS, so that, at minimum, whether detection was on or off can be read off
the record.

**Meaning of "calls made into AWS" used here:** every request the page sends to AWS (our API, and
the uploads and downloads that go straight to S3), plus every call our Lambda functions make to S3
and DynamoDB, counted per request.

**Goal, as a test:** after one session on the `dev` stack, one command prints a single timeline in
which Claude can read, without asking: each click and what it was on; each tab change; each
message the page showed (load progress, errors, "Saved."); each request to the API and to S3, with
its result and time; whether the upload asked for detection and each detection page; each flag
save and which message it was for; and each processing run with its upload.

## §1 Three sources, one session ID

| Source | Records | Where it goes |
|---|---|---|
| A. API Gateway access log | every request that reaches the API, **including ones refused before our code runs** (401s, CORS pre-flights): time, method, route, status, total and integration time, request ID | new CloudWatch log group `/timeline/<stage>/api-access` |
| B. Our Lambdas | one line per API request (route, status, duration, user, session ID, request-specific facts, counts of S3/DynamoDB calls by operation and their failures); one line per processing run | each function's log group |
| C. The page | clicks, changes to checkboxes and the file chooser, tab changes, messages it showed, and its own requests (to the API and to S3), each with time and outcome | sent in batches to a new `POST /activity` route; written as lines in the API's log by source B |

**The session ID** ties them together: the page makes a random ID when it opens
(`crypto.randomUUID()`), sends it as an `x-timeline-session` header on every API request, and puts
it on every recorded event. HTTP API access logs can't read request headers, so source A is joined
to B by API Gateway's request ID, which B logs too.

**Why the page records its own requests too:** the file upload and the export download go straight
to S3 through signed links, never through the API, so neither A nor B sees them. S3's own request
logging (server access logs, or CloudTrail data events) could, but adds a bucket or a trail and is
delivered with minutes to hours of delay; the page's record is immediate and costs nothing extra.
Not chosen; revisit if the page's record is ever in doubt.

## §2 Source A: API Gateway access log

In [infra/template.yaml](../../infra/template.yaml), on `HttpApi`: `AccessLogSettings` with
`DestinationArn` pointing at a new `AWS::Logs::LogGroup` (`ApiAccessLogGroup`) and a JSON `Format`
of `$context.requestId`, `$context.requestTime`, `$context.httpMethod`, `$context.routeKey`,
`$context.path`, `$context.status`, `$context.responseLatency`, `$context.integrationLatency`,
`$context.authorizer.error`, `$context.error.message`. No IP address and no user agent: they
identify the user's machine and add nothing the checks need.

## §3 Source B: our Lambdas

**Libraries:** `tracing` (MIT), `tracing-subscriber` (MIT) with its `json` feature, and
`tower-http`'s `trace` feature (MIT; `tower-http` is already a dependency,
[timeline-api/Cargo.toml:20](../../backend/timeline-api/Cargo.toml#L20)). `lambda_runtime`
(already used) offers a ready-made subscriber setup for Lambda (`lambda_runtime::tracing`), to be
checked first; if it suits, use it rather than configuring `tracing-subscriber` by hand.

1. **One line per API request**, from a `tower-http` `TraceLayer` on the router in
   [app.rs](../../backend/timeline-api/src/app.rs): `kind: "api_request"`, method, route
   template (`/conversations/{id}/messages/{id}/flags`, not the raw path), status, duration,
   user (Cognito `sub`), session ID, API Gateway request ID.
2. **Facts specific to a request**, added to that line by the route itself:
   - `POST /uploads`: upload ID, declared size.
   - `POST /detect`: offset, limit, conversations in the page, flags set.
   - `PATCH …/flags`: conversation and message IDs, the three values saved, handle accepted or
     refused.
   - `GET /export`: conversation count, response size.
3. **Counts of our own calls to AWS.** For each request, how many calls to each S3 and DynamoDB
   operation (e.g. `dynamodb.PutItem: 112, dynamodb.Query: 3`) and how many failed. The AWS SDK
   reports each operation through `tracing`; a small `tracing` layer counts them per request.
   **To verify first:** the span names and fields the SDK version in use emits. If they don't
   name the operation reliably, fall back to counting in our storage adapters
   ([timeline-storage/src/dynamo/](../../backend/timeline-storage/src/dynamo/),
   [s3.rs](../../backend/timeline-storage/src/s3.rs)), and say so.
4. **One line per processing run** in
   [s3_trigger.rs](../../backend/timeline-api/src/s3_trigger.rs): upload ID, attempt number,
   size, conversations and messages stored, time per stage, outcome, AWS call counts. The failure
   recorder ([failed_upload.rs](../../backend/timeline-api/src/failed_upload.rs)) logs one line
   per upload it marks failed.
5. The existing `eprintln!` lines stay as they are, now with the request's ID beside them where a
   request is in progress; converting them is not part of this plan.

## §4 Source C: the page

**New modules:**
- [frontend/core/activity-event.js](../../frontend/core/) (pure, no browser access): builds an
  event from what happened. Describes an element by its `id`, its `data-*` attributes
  (`data-id`, the message ID on review rows), its tag, and its label (button text, a checkbox's
  label, a tab's name), capped at 80 characters with control and invisible characters removed.
  **Never** text from table cells, message bodies, file contents or tokens: an element inside the
  review table or the timeline is described by its message ID and column only.
- [frontend/infra/activity-recorder.js](../../frontend/infra/): keeps events in memory and sends
  them to `POST /activity` every 5 seconds **if there are any** (an idle page sends nothing), and
  when the page is hidden (`fetch` with
  `keepalive`, since `navigator.sendBeacon` can't send the `Authorization` header).

**Which user actions: every kind the page's JavaScript responds to** (the user's direction,
2026-10-02: page actions, not mouse movement or scrolling). Inventory read from the code on
2026-10-02 (every `addEventListener` and `on…=` handler outside tests): the page listens for
exactly four kinds of user action, `click` (31 handlers), `change` (6: checkboxes, the review
filter, the file chooser), `input` (2: the conversation search and the review search, one event
per keystroke) and `hashchange` (1: the address's `#` part, which selects the tab). Nothing
listens for key presses, scrolling, mouse movement, dragging or pasting. So one capture-phase
listener per kind, on the document or window, records `click`, `change` and `hashchange` without
touching existing handlers; `input` is the exception, recorded only on submit (below). A test re-runs the inventory and fails when a handler for a
new kind of event appears, so the recorder can't silently fall behind the page.

**What is recorded**, each with time, session ID and the current tab:
- `click`: every click.
- `change`: checkboxes and selectors with their new value; the file chooser with the file's size
  and extension only, not its name, which can be personal.
- `submit`: a text box's contents, **only when the user submits it**: presses Enter in it, or a
  form's own `submit` happens (the user's direction, 2026-10-02: a pause in typing is not an
  action). Recorded with the text (capped at 80 characters) and the box's `id`. Typing itself is
  not recorded. **Consequence today:** neither search box has a submit action (both filter as
  you type, and the page has no `<form>`), so searches are not recorded at all unless the user
  presses Enter in one. The clicks that follow a search are still recorded, with message IDs.
  This needs one listener the page doesn't otherwise have: Enter key presses in text boxes, and
  nothing else about the keyboard.
- `view`: tab changes (`hashchange` and the router,
  [frontend/ui/router.js](../../frontend/ui/router.js)).
- `shown`: every message the page shows, by recording inside the setters in
  [status-indicators.js](../../frontend/ui/widgets/status-indicators.js) (`setLoadStatus`,
  `setSaveStatus`, the load-progress labels, `failLoadProgress`, `showRestoredNotice`), whether
  the sign-in label says signed in or out (not the email address it shows), and the error area. These are fixed wordings plus server error text, capped.
- `request`: each request the page makes, through one new `apiFetch` in
  [api-client.js](../../frontend/infra/api-client.js) that also adds the `Authorization` and
  session headers. The seven call sites in [load-flow.js](../../frontend/ui/load-flow.js) and
  [api-client.js](../../frontend/infra/api-client.js) move to it; the S3 upload
  (`putWithProgress`) and the S3 download record the same event. Records method, route (S3
  links reduced to `s3 PUT raw/…`, with the signature removed), status, duration, bytes. The
  `/detect` events also record offset and limit; the upload start records whether the scan box
  was ticked.

**Before sign-in:** events wait in memory and are sent once a sign-in exists. If the user never
signs in, they are never sent (and the page then can't call the API either).

**If sending fails:** the page keeps up to 2,000 events, drops the oldest beyond that, reports the
failure in the browser console (`console.warn`, with the status), and includes "N events dropped"
in the next batch that gets through. Recording never blocks or breaks the page.

**Turning it on and off:** the deploy config the page already reads
([frontend/deploy-configs/](../../frontend/deploy-configs/)) gains `recordActivity: true|false`,
written by [scripts/write-deploy-config.sh](../../scripts/write-deploy-config.sh) from a new
stack setting `RecordActivity` (default `on` for `dev`). With it off, nothing is recorded or sent.
Local development (no deploy config) records to the browser console only.

## §5 The `POST /activity` route

In a new [routes/activity.rs](../../backend/timeline-api/src/routes/): requires a sign-in like
every route; accepts up to 200 events of at most 4 KB each; each event's `kind` must be one of the
five above (anything else is refused with 400, naming it); every text field is capped again. Each
accepted event becomes one log line `kind: "page_event"` with the user's `sub` and the session ID.
Nothing is stored in DynamoDB. Answers 204. CORS `AllowHeaders` gains `x-timeline-session`.

## §6 How long records are kept

**The user's direction (2026-10-02):** keep at least 24 hours of history and delete anything older
than a day, daily, to avoid paying to store clicks; keep more only for a concrete reason that
doesn't cost much.

The functions' log groups were created by Lambda on first run and keep logs forever. The template
gains `LoggingConfig.LogGroup` for each function, pointing at new log groups it owns
(`/timeline/<stage>/api`, `/timeline/<stage>/process-upload`,
`/timeline/<stage>/record-failed-upload`, plus §2's `/timeline/<stage>/api-access`), each with
CloudWatch's `RetentionInDays` from a new stack setting `LogRetentionDays`, **default 1**.
CloudWatch then deletes older records itself; nothing of ours runs to delete them. (AWS's
documentation describes retention as a minimum, with deletion happening some time after; how soon
after is not checked. Records are therefore kept *at least* a day, never less.) The old groups
stay (holding the 2026-10-02 checks' evidence) until deleted by hand.

**A concrete reason to keep more, and its cost** (figures from §10): an analysis made more than
a day after a session, e.g. a check run on a Friday and analysed on a Monday, would find nothing.
Keeping 7 days instead of 1 costs about **$0.005 a month** on a heavy-use estimate, because
CloudWatch charges $0.03 per GB per month to store logs and a heavy week is about 0.2 GB. Storage
is not where the cost is; taking the records in is (§10). **Question for the user** below.

## §7 Reading it back: `scripts/activity-timeline.sh`

`scripts/activity-timeline.sh <stage> [--since 30m] [--session <id>]` reads the four log groups
with `aws logs filter-log-events`, joins API Gateway's lines to the API's by request ID, sorts
everything by time and prints one line per event:

```
19:11:24.112 page   click     #loadBtn "Load"  (scan box: ticked)
19:11:24.160 page   request   POST /uploads → 200 41 ms
19:11:25.900 page   request   s3 PUT raw/… → 200 1.7 s 60.6 MB
19:11:26.020 lambda process   upload 7f3… attempt 1 → stored 812 conversations, 6.1 s, dynamodb.PutItem 1,624
19:11:36.400 api    request   POST /detect offset 0 limit 32 → 200 3.2 s, dynamodb.PutItem 410 (session 5c1…)
```

Written in Python (standard library only) and run through the `aws` command, so no new
dependency. The deployment analysis documents cite its output from now on.

## §8 Tests

- **Rust (public API only):** a request through the router produces one `api_request` line with
  route template, status, session and request IDs (a test `tracing` writer captures the output);
  `/detect` and the flag save add their facts; AWS-call counting against DynamoDB Local and the
  S3 stand-in already used by `timeline-storage`'s tests; `POST /activity` accepts a valid batch,
  refuses 201 events, an oversized event and an unknown kind, each with 400 and the reason, and
  refuses no sign-in with 401; a processing run logs its line, including a failed attempt.
- **Template** (text checks in the style of
  [template_event_logging.rs](../../backend/timeline-api/tests/)): access log settings and
  format, the four log groups with retention, `LoggingConfig` on each function, the CORS header,
  the two new settings. `scripts/check-template.sh` passes.
- **Frontend unit:** the handler inventory (§4): scans the frontend source for `addEventListener`
  and `on…=` handlers and fails if any listens for a kind of event the recorder doesn't record;
  typing in a text box records nothing, Enter in it records one `submit` with its text, and Enter
  elsewhere records nothing; element descriptions (an id; a label; a review-row checkbox gives message ID
  and column and **no cell text**; caps and character stripping); the recorder's batching,
  2,000-event cap and "dropped" count, waiting for sign-in, and sending nothing when off.
- **Browser** (against the local backend, which logs `/activity` the same way): sign in, tick
  the scan box, load the synthetic export, approve a row, change tabs; the backend's log then
  holds, in order, the clicks, the `/uploads`, the S3 PUT, each `/detect`, the flag save with its
  message ID and "Saved.", all with one session ID; and no line contains any message text from the
  synthetic export.
- **The timeline script:** against a stand-in `aws`, in the style of
  [test-deploy-scripts.sh](../../scripts/test-deploy-scripts.sh), with lines in the documented
  formats: joins by request ID, sorts, filters by session.
- **On AWS (end to end, after deploying):** rerun D4, D5 (detection off, then on) and D6, then
  `scripts/activity-timeline.sh dev --since 30m`. Done means Claude can state from its output
  alone, with no questions: the refused requests and why; which upload had detection on; every
  detection page's time; which message's flag was saved and the result. The rerun is written up
  in the deployment-checks analysis. Until that run, the status is "code-level only, end-to-end
  TBD".

## §9 Privacy

- **Privacy:** records hold user IDs, message and conversation IDs, flag values, file sizes,
  button labels, the page's own messages, and **text-box contents the user submitted with Enter**
  (capped at 80 characters), which may contain words from their conversations. No message text, file names,
  file contents, tokens, IP addresses or email addresses. With a 1-day retention, submitted text
  is gone about a day after submission. This is a development stack with one user; before anyone
  else uses it, recording should be reviewed (and `RecordActivity` defaults `off` for any stage
  but `dev`).

## §10 Impact on response time and cost

Prices are AWS's own, read from its price list (`aws pricing get-products`, us-east-1) on
2026-10-02. Volumes are **estimates**; event sizes are guesses until the code exists, and are
measured in §8's end-to-end run. The free allowances AWS gives each month are left out, so these
are upper figures.

| Price (us-east-1, on demand) | |
|---|---|
| CloudWatch Logs, taking in records (Standard class) | $0.50 per GB |
| CloudWatch Logs, storing records | $0.03 per GB per month |
| API Gateway HTTP API requests | $1.00 per million |
| Lambda requests (Arm) | $0.20 per million |
| Lambda running time (Arm) | $0.0000133334 per GB-second |

**Volumes measured today, for scale:** the API ran 301 times in the 24 hours to 19:41 UTC on
2026-10-02, a day of active testing; the peak was 2 copies of it running at once. Its log lines
today average 90 bytes.

**Estimated added volume, heavy day** (8 hours of reviewing, one action every 3 seconds, every
action a flag save; well above today):

| Source | Count per day | Size each (guess) | Per day |
|---|---|---|---|
| Page events (actions, messages shown, requests) | ~30,000 | 400 bytes | 12 MB |
| API request lines (B), with AWS call counts | ~10,000 | 500 bytes | 5 MB |
| API Gateway access lines (A) | ~16,000 (incl. batches) | 350 bytes | 5.6 MB |
| `POST /activity` batches: Lambda's own start/end/report lines | ~5,800 | 400 bytes | 2.3 MB |
| **Total** | | | **~25 MB** |

**Cost, heavy day every day for a month:**

| Item | Month |
|---|---|
| Taking in ~0.75 GB of records | $0.38 |
| Storing them, 1-day retention (~25 MB kept) | under $0.001 |
| Storing them, 7-day retention (~0.18 GB kept) | $0.005 |
| ~174,000 `POST /activity` requests through API Gateway | $0.17 |
| the same through Lambda: requests, plus ~10 ms each (a guess) at 512 MB | $0.05 |
| **Total** | **about $0.60 a month** |

A day like today's (301 API requests) is roughly a fiftieth of that. **The expensive choice would
have been recording every keystroke** in the search boxes; recording text only when submitted (§4)
keeps it off this table. Mouse movement and scrolling, excluded by the user, would have been the largest
items by far.

**Response time** (expected; measured after building, see the acceptance check below):
- *The page:* each recorded action does a few small object operations and adds to an in-memory
  list; sending happens in the background every 5 seconds and doesn't wait for anything the user
  is doing. Expected to be imperceptible.
- *The API:* one more JSON line per request, plus counting the AWS calls it makes. A detection
  page makes hundreds of DynamoDB calls; counting adds a little work to each. Expected to be well
  under 1% of a 3-second detection page, but this is the part most worth measuring.
- *API Gateway's access log:* written by AWS outside the request; expected to add nothing.
- *Sharing Lambda capacity.* This account may run at most **10 Lambda copies at once**, across
  all its functions (read with `aws lambda get-account-settings`; AWS's usual default is 1,000 and
  some new accounts start lower; that is from memory, not checked). Each batch occupies an API copy for ~10 ms every 5 seconds,
  about 0.2% of one copy's time. If all 10 are busy, a request is refused (429). The batch's
  failure path (§4) keeps the events and retries; a refused *user* request would be visible, which
  is why the acceptance check below looks for any 429. Raising the limit is a request to AWS
  (believed free; not checked), outside this plan.

**Acceptance check for response time,** in §8's end-to-end run, against today's measurements as
the baseline (detection pages 3.0–4.4 s, processing 6.1–6.6 s for the same 60.6 MB export,
[deployment-checks analysis](../analysis/2026-10-02-deployment-checks-status.md)): with recording
on, run the same upload with detection, then compare each function's `REPORT` durations, and the
page's own measured request times, against the baseline; and confirm no 429s. **Proposed
threshold: median detection page and processing time no more than 5% slower.** If it is slower,
report it and stop, rather than tune it quietly.

## Questions for the user

1. **Retention:** 1 day (your direction; the default) or 7 days (about half a cent a month more on
   the heavy estimate, so a session can still be analysed after a weekend)?
2. **Response-time threshold:** is "no more than 5% slower" the right line?

## Self-critique log

### C1 [RESOLVED]: S3 uploads and downloads would be invisible
Original concern: A and B see only the API; the upload and export download go straight to S3.
**Resolution:** the page records its own S3 requests ([§4 (line 131)](#L131)); S3's own logging
was considered and not chosen ([§1 (line 40)](#L40)).

### C2 [RESOLVED]: refused requests never reach our code
Original concern: a 401 from API Gateway's authorizer, or a CORS pre-flight, produces no Lambda
line, so D2 and D4 would stay unprovable. **Resolution:** API Gateway's access log
([§2 (line 46)](#L46)), which records them with the authorizer's error.

### C3 [RESOLVED]: recording could leak message content into logs
Original concern: describing clicked elements by their text would copy message text from the
review table into CloudWatch. **Resolution:** elements inside the review table and the timeline are
described by message ID and column only, and a browser test checks that no synthetic message text
reaches the log ([§4 (line 91)](#L91), [§8 (line 218)](#L218)).

### C4 [RESOLVED]: the existing log groups can't be given a retention by a template
Original concern: Lambda created them on first run; a template that declares groups with the same
names fails because they already exist. **Resolution:** new, template-owned groups through
`LoggingConfig` ([§6 (line 167)](#L167)); the old groups are left in place.

### C5 [OPEN]: counting the SDK's calls depends on its tracing output
The per-request AWS call counts rely on span names the AWS SDK emits, not yet read. **Mitigation in
plan:** a named fallback, counting in our storage adapters ([§3 (line 73)](#L73)). **Open:** decided
by the first step of coding, reading the SDK's spans; the choice is reported to the user.

### C6 [OPEN]: events from a page closed before sign-in are lost
**Mitigation in plan:** stated in §4. **Open:** if a check ever needs pre-sign-in clicks, an
unauthenticated route would be needed, which brings abuse limits; not proposed now.

### C7 [OPEN]: moving every request to `apiFetch` touches every request site
Seven call sites move to one helper; a mistake there breaks the page's requests.
**Mitigation in plan:** the existing browser tests cover upload, export, detection and flag saves,
and run unchanged. **Open:** none expected; revisit if any browser test needs changing.

### C8 [RESOLVED]: the sign-in label would put the email address in the logs
Original concern: §4 recorded the sign-in label, "Signed in as <email>", contradicting §9's "no
email addresses". **Resolution:** only signed in or out is recorded ([§4 (line 130)](#L130)).

### C9 [RESOLVED]: recording every keystroke would dominate the cost
Original concern: the two search boxes respond to every keystroke; recording each would multiply
the page's events. First resolution: record once typing paused for 1 second. **User pushback
(2026-10-02):** a pause is not an action; record a text box only when submitted (Enter, or a
form's submit). **Resolution:** done ([§4 (line 117)](#L117)), with a test; typing adds nothing to
the cost table ([§10 (line 267)](#L267)).

### C10 [OPEN]: the account's limit of 10 Lambda copies at once
Background batches share that limit with the user's own requests. **Mitigation in plan:** batches
are short and only sent when there are events; a refused batch is kept and retried; the
acceptance check looks for any 429 ([§10 (line 302)](#L302)). **Open:** if any 429 appears in that
run, raise the limit with AWS before recording is left on.

### C11 [RESOLVED]: submitted text may contain words from the user's conversations
Original concern: recorded search text could carry conversation words into the logs.
**Resolution:** only text the user submits is recorded, capped at 80 characters, kept about a day
([§9 (line 236)](#L236)); with today's page, that is only Enter in a search box.

### C12 [OPEN]: the cost figures are estimates
Event sizes and counts are guesses until the code exists. **Mitigation in plan:** the end-to-end
run measures the real bytes per session. **Open:** if the measured bytes per action are more than
twice the 400-byte guess, the cost table is redone and shown to the user before recording is left
on.
