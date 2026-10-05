# Activity recording on AWS: the end-to-end run, 2026-10-05

The [activity plan](../plans/2026-10-02-activity-instrumentation.md)'s §8 "On AWS" run: deployment
checks D4, D5 (detection off, then on) and D6 done on the `dev` stack, then read back with
`scripts/activity-timeline.sh dev`. Its test of done: Claude can state, from that output alone,
the refused requests and why, which upload had detection on, every detection page's time, which
message's flag was saved and the result; and server timings are no more than 5% slower than
2026-10-02's, with no 429s.

**Headline: met, from the records alone.** The user's later session ("and others") had no page
records at the first two reads; the tab was still open and idle, which by design sends nothing.
Its records arrived once it was used again and then closed (see "The gap, resolved"), so both
sending paths, at a quiet moment and on closing, worked on AWS.

## Setup

- **What was deployed:** the stack as the page-hosting session deployed it on 2026-10-02 at
  22:01 UTC. The activity function, the five `/timeline/dev/*` log groups (7 days) and
  `RecordActivity: on` were live. The API's log lines already carried `aws_failures` and
  `aws_errors`, and no backend or template commit came after that deploy, so nothing was
  redeployed. The page was served from the working tree at commit `ac0c2e9` (no uncommitted
  changes), through the user's Tailscale address with `?deploy=dev`.
- `scripts/write-deploy-config.sh dev` wrote `recordActivity: true`, `pageVersion: "ac0c2e9"`.
- The user was asked to: sign in; load the export with the scan box unticked; "Load a different
  file" and load it again with the scan box ticked; approve one row; wait 5 s and close the tab.
  Claude sent three refused requests itself at 14:34:54.

## What the records say (all times UTC)

Read with `scripts/activity-timeline.sh dev --since 10m` at 14:42 and again at 14:43 (293 lines).
Session `06cf9fe9…` is the asked-for run; `84e6da74…` is the user's later one.

| Check | From the records | Result |
|---|---|---|
| D4: refusals | 14:34:54–55, API Gateway's request log only (our code never ran): `GET /conversations` with no token → 401 "missing: token not provided"; with `Bearer nonsense` → 401 "invalid_token: token contains an invalid number of segments"; `POST /activity` with no token → 401 "missing" | **Shown.** Another pool's token was not re-sent this run (it was on 2026-10-02) |
| D5: which upload had detection on | page `POST /uploads scan off` at 14:36:10 (upload `ff725b23…`); page click on `input#autoDetectCheckbox` → `true` at 14:37:11, then `POST /uploads scan on` at 14:37:13 (upload `4081eecd…`) | **Shown** |
| D5: processing | `processing_run` lines: `ff725b23…` ready, attempt 1, 63,516,906 bytes, 117 conversations, 538 reviews, 6.4 s; `4081eecd…` 5.7 s; same counts; no AWS failures or retries | **Shown** |
| D5: every detection page | 24 `POST /detect` lines for session `06cf9fe9…`, 14:37:45–14:39:05, offsets 0–115, limit 5, 18–366 messages each, 2.9–4.2 s our code (3.0–4.3 s at API Gateway), each with its DynamoDB writes | **Shown**; all under 30 s |
| D6: the flag save | 14:39:46 page click `button` message `83\|2026-07-01T02:11:28…` column `approve`; server `PATCH …/flags` message `019f1b72-13bf-70e7-aa30-fac7f9a61b3b`, all three false, `handle accepted` → 200; page shown `saveStatus: save.saved` | **Shown** |
| Speed | Lambda's own run times. Detection pages: 2026-10-02 median 3,265 ms (24 pages); now 3,139 and 3,151 ms (24 each). Processing: 2026-10-02 median 6,356 ms (2 runs); now 5,677 ms (3 runs). Memory at most 354 MB of 512 | **Not slower** (why faster is not traced) |
| 429s | API Gateway's request log, 14:33 onwards: 0 | **None** |

The page's own records arrived in four batches (14:37:13, 44 and 1 events; 14:39:38, 43; 14:40:12,
8), each answered 204 by the activity function; every API request had its API Gateway line by the
second read, except the last (14:42:33).

## Found from the records (not asked for)

- **The page's quiet restore ran alongside the new upload.** Signed in at 14:35:59; the restore's
  `GET /export` (12.3 s) and its 63.9 MB download (25.1 s) ran during the first upload's PUT
  (26.0 s); "restored" and "flags.loaded 550" were shown at 14:36:37, mid-upload. Known: the
  migration plan's follow-up "a restore screen with a Stop button".
- **"Load a different file" did not stop a load in progress.** Clicked at 14:36:42, before upload
  `ff725b23…` finished processing (14:36:43); the old load carried on behind the load screen:
  `GET /export` at 14:36:55, its download, "progress.preparing" and "flags.loaded 550" at
  14:37:03. Not on any list yet.
- **Session `84e6da74…` (14:40:15 onwards):** a restore (`GET /export`), a new upload
  `b233ed47…` (processed 5.7 s, attempt 1), a full detection pass (24 pages, 14:41:03–14:42:23),
  and **three flag saves during that detection pass**, i.e. in the restored view while a new load
  ran (the same known restore problem). From the page's records (arrived later): Approve on a
  message at 14:41:04; the `caps` box set to false at 14:41:10; the `critical` box set to true at
  14:41:35; each answered `save.saved`, and the next load showed `flags.loaded count 553` (550
  before). The upload was `POST /uploads scan on` from a Load click at 14:40:29 with no file
  choice or scan-box click recorded in that session: after the reload the browser kept the chosen
  file and the ticked box (inferred: no `change` records, yet a 63.5 MB PUT and `scan on`).
- **The processed export is rebuilt for every load and restore:** five `GET /export` calls, each
  9.9–11.9 s of our code and 2,227 DynamoDB reads, and each followed by a 63.9 MB download.
- The first page action was **Load with no file chosen** (14:36:03, `load.choose_file`), and the
  user clicked timeline bars (`div column bar:181`, `bar:104`); the records name neither the
  conversations nor any wording.

## The gap, resolved

At 14:43:02, session `84e6da74…` had 37 server lines and no page records; its last request was the
export at 14:42:33. By design (the plan's §4) a page sends only at a quiet moment after recording
something, or when it is hidden or closed, so an open, idle page sends nothing; the records alone
couldn't tell an open tab from a failed send-on-close. The user then closed the tab, and a read at
14:46:49 showed:

- **14:43:20:** a click on the Conversations tab, the first record after a quiet spell, started a
  send: 69 records (204), then 2 more recorded while it was sending (204).
- **14:43:52:** the tab closing sent the last 4 (two tab views, a tab click, a click on the
  header) (204).

So the tab was open at 14:43:02. The page's clock ran slightly ahead of AWS's: the click that
started the 14:43:20 send is stamped 14:43:20.817 by the page, after the send's 14:43:20.761 at
the activity function.
