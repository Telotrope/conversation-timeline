# Waits after "load only what the page shows": local measurements (AWS not yet measured)

Step 12 of [the plan](../plans/2026-10-06-load-only-what-the-page-shows.md#L751) asks for §9's waits
and §8b's longest gap between moves of the progress bar, measured locally and, after a deployment,
on AWS with processing at 512 MB and 1,769 MB.

**Headline.**
- **Locally, every wait in §9 is now under 3 s.** Uploading the user's 63.5 MB export with the scan
  ticked takes 3.4–4.4 s from Upload to Describe; §9 estimated about 3.5 s. The longest the bar went
  without moving was 0.58 s, during preparing.
- **AWS: not measured.** The deployment was not made. The only stack, `timeline-dev`, serves the
  public address, and deploying it would change the live site (see "AWS" below). The 512 MB and
  1,769 MB processing runs are therefore not done either. Every AWS column of §9 is still an
  estimate.

## How it was measured

- [e2e/measure-waits.js](../../e2e/measure-waits.js) drives the page in headless Chrome on this
  machine:
  - It signs in with local development's sign-in and uploads
    [real-flags.json](../../real-flags.json) (63,516,906 bytes, the export §9's figures come from)
    with the scan ticked, then presses Done on Describe.
  - It then opens Review, searches for "the" with no other filter, and turns on Claude's replies.
  - It opens each of the five analyses, then the two server analyses again (now saved), and
    downloads the annotated file.
- **The server:** a release build of `timeline-api` on 127.0.0.1:3917, with in-memory storage and
  the production time limit (9 s of work per request, no `TIMELINE_WORK_BUDGET_STEPS`).
- **The page:** served from the working tree at commit `22b1d15`, plus the uncommitted
  measurement script.
- **A wait:** from the action (click, typing) to what ends it: the next stage's words on the bar,
  the Describe page, the loading box closing, the first rows of Review, the drawn chart, or the
  download.
  - The upload's four stages are split at the first bar words of the next stage ("Sending your
    file", "Processing on the server", "Starting the scan"), ending at Describe.
  - The search wait includes the page's 300 ms pause after typing, since the page asks only then.
- **A move:** a change of any bar's fill width, or of its words with their clocks taken off
  ("· 12 s so far", and the processing wait's "— 1m 5s"). A clock alone is not progress (§8b).
- **The longest gap:** the longest time between a wait's start, each move, and its end.
- **Runs:** 2, 3 and 4, each as a new user. Run 4 used a freshly started server (see "The scan").
  - Run 1 is left out: the script measured "opening the timeline" before the loading box had
    opened, and cleared the search before the page had asked for it (the server's log shows no
    search request). The script was fixed before run 2.
  - **Local only, over the loopback address:** sending moves no bytes over a network, so "sending"
    here says nothing about the user's connection.

## Results (seconds; each cell is the wait, then its longest gap without a move)

| Wait | Run 2 | Run 3 | Run 4 | §9 "After, local" estimate |
|---|---|---|---|---|
| Preparing the file in the browser | 2.95 / 0.57 | 2.48 / 0.58 | 2.19 / 0.57 | 1.5 |
| Sending the file (2,805,384 bytes; locally this includes processing, see below) | 0.53 / 0.41 | 0.50 / 0.37 | 0.44 / 0.32 | 1 (at 2.5 MB/s) |
| Processing, as the page sees it | 0.01 / 0.01 | 0.01 / 0.01 | 0.01 / 0.01 | 0.5 |
| Scanning | 0.92 / 0.51 | 0.92 / 0.50 | 0.77 / 0.49 | 1 |
| **Upload to Describe, in all** | **4.42** | **3.92** | **3.41** | about 3.5 (the four rows above added up) |
| Opening the timeline (Done on Describe to the loading box closing) | 0.49 / 0.21 | 0.39 / 0.11 | 0.46 / 0.18 | 0.1 |
| A page of Review, to its first rows | 0.34 / 0.20 | 0.44 / 0.27 | 0.22 / 0.11 | 0.1 |
| A search with no other filter, to its first rows | 0.60 / 0.34 | 0.62 / 0.34 | 0.51 / 0.34 | 0.2 |
| Claude's replies on, to the first reply shown | 0.30 / 0.17 | 0.34 / 0.18 | 0.21 / 0.13 | 0.2 |
| Friction ranking (in the page) | 0.17 / 0.04 | 0.16 / 0.06 | 0.14 / 0.06 | instant |
| Session length against flag rate (in the page) | 0.15 / 0.10 | 0.12 / 0.08 | 0.17 / 0.11 | instant |
| Idle time before a session (in the page) | 0.09 / 0.08 | 0.09 / 0.07 | 0.10 / 0.08 | instant |
| Flag rate over time (server), first time | 0.24 / 0.17 | 0.26 / 0.22 | 0.17 / 0.12 | 0.3 |
| Time of day and day of week (server), first time | 0.20 / 0.20 | 0.26 / 0.26 | 0.17 / 0.17 | 0.3 |
| Flag rate over time, saved | 0.10 / 0.07 | 0.09 / 0.07 | 0.09 / 0.07 | 0.05 |
| Time of day, saved | 0.07 / 0.04 | 0.06 / 0.04 | 0.06 / 0.03 | 0.05 |
| The annotated download (8,214,245 bytes, in three parts) | 0.66 / 0.25 | 0.79 / 0.34 | 0.65 / 0.26 | 3 |

**Review's count was final when its first rows arrived** (0.01 s after, in every run): with the
9 s time limit, the whole walk over 301 sessions fits in one request locally. The "at least N"
count, and searches growing part by part, therefore weren't exercised here. They were exercised in
the browser test ["the scan's bar and Review's count grow part by part"](../../e2e/views.spec.js),
which runs with a 20-row limit per request.

**Against §8b's rule** (the bar moves at least every 10 s): no gap locally exceeded 0.58 s. Locally
every wait is short, so this says little about the rule. AWS, with about 5 times slower server
work and a real connection, is where the rule is tested.

## What the server's log says about the same runs

From the release server's request log (`processing` facts on the local upload request; `ms` on each
request):

| Run | Processing (in the upload request) | of which parsing | Decompressing | Rows written | The scan, on the server |
|---|---|---|---|---|---|
| 1 | 551 ms | 369 ms | 67 ms | 8 ms | 291 ms |
| 2 | 501 ms | 333 ms | 63 ms | 8 ms | 404 ms |
| 3 | 465 ms | 334 ms | 38 ms | 9 ms | 414 ms |
| 4 (fresh server) | not read | | | | 265 ms |

- **Processing.** Locally, the server processes the file inside the request that receives it.
  The page's "processing" wait (0.01 s) is therefore only its first status check; the processing
  itself is inside "sending". Every run read the slimmed file as 9,328,868 bytes (2,805,384
  compressed). Each stored 118 conversations, 301 sessions, 4,444 messages (2,222 of yours) and 536
  reviews.
- **536 reviews, not 538: a difference not yet traced.** The processing before this work stored
  538 from the same file ([2026-10-05 run](2026-10-05-activity-recording-aws-run.md#L40)). Counted
  in the raw file with Python:
  - It holds 561 reviews. The old processing dropped 23 of them along with the repeated sends it
    removes.
  - 27 reviewed messages lie off the path to their conversation's latest message, so pruning
    (§4d) does not keep them on that conversation's rows.
    - 22 are in "Starting a government contracting business". 118 conversations were stored from
      117, so one branch was likely kept as its own conversation, and these 22 are probably in it
      (inferred, not checked).
    - The other 5 are in branches pruned to notes. All 27 reviews are "none of the three flags".
  - I have not worked out how those 5 and the 23 combine to give exactly 2 fewer. The cheapest
    test: count the stored reviews per conversation against the raw file's, conversation by
    conversation.
  - Whether a review on a pruned message should be kept is a question for the user.
- **The scan.** It took 291 ms on the server with one user stored, and 404 and 414 ms with one and
  two earlier users' rows in the same in-memory store. A freshly started server (run 4) brought it
  back to 265 ms. That supports, but does not prove, that the local in-memory store slows as other
  users' rows accumulate; DynamoDB keeps each user's rows apart, so this would not carry over to
  AWS.
  - The page's scanning wait (0.77–0.92 s) is longer than the server's scan. The rest is the status
    checks and the reads before Describe opens. I have not traced which of those differed between
    run 4's 0.77 s and runs 2 and 3's 0.92 s.

## AWS: not measured, and why

On 2026-10-06 the dev stack and its settings did not match the repository:
- `timeline-dev` is the only stack, and its CloudFront distribution `E6MWRB0WWMX8K` serves
  **howangryami.telotrope.ai**, the public address, with the public certificate. The address
  resolves to that distribution.
- The committed [infra/samconfig.toml](../../infra/samconfig.toml) gives dev the address
  **dev.howangryami.telotrope.ai** and a different certificate. It was committed after the last dev
  deploy (2026-10-02 22:01 UTC).

A dry run of `scripts/deploy.sh dev` (answered "no"; the change set was deleted) showed the
expected changes: the MessageFlags table removed (counted at 0 items first), the new PUT route, the
functions updated. It also showed the sign-in client and the distribution changing to the new
address.

So a deploy now would either:
- **with the committed settings:** move the site off the public address; or
- **with today's address kept:** replace the API that the page live at the public address uses.
  That page still asks for the removed timeline download, so it would break until the new page is
  published there too.

Both change a public site, so neither was done without the user. The AWS measurements, at 512 MB
and 1,769 MB, are still open. The same script can make them once the deployment is settled and a
sign-in is available to it. On AWS the page signs in through Cognito, Amazon's sign-in service,
and the script has only local development's sign-in so far.
