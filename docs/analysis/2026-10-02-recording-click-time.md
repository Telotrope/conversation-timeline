# Why recording a click sometimes takes over 1 ms

The activity plan ([plan](../plans/2026-10-02-activity-instrumentation.md)) limits the time recording
adds to each click to 1 ms. The browser test `e2e/activity.spec.js` ("recording a click on the review
table takes under 1 ms") times the recorder over 1,000 clicks fired back to back on the review table.
It fails intermittently. These are the measurements made on 2026-10-02 to find out why. All runs are
local (Chrome, the local backend); nothing here ran on AWS.

## Is it the same click every time?

No. Eight runs of the real test, machine load average 0.24:

- **Click 0** was slow in every run, 0.5–0.8 ms (under the limit).
- **One click of 1.5–1.6 ms** appeared in 4 of the 8 runs, at positions 597, 651, 652 and 673; no
  other click reached 0.3 ms.

Average cost per click across runs: 22–46 µs.

## Hypotheses and tests

| # | Hypothesis | Test | Result |
|---|---|---|---|
| H1 | The browser's memory clean-up runs during the slow click | Read the page's memory use (`performance.memory`, with Chrome's `--enable-precise-memory-info`) before and after every click | **Supported.** In the one probe run with a slow click (position 526, 2.0 ms), memory dropped by 1.7 MB during it, the largest drop in four runs; smaller drops (0.1–1.0 MB) in other clicks took ≤ 0.2 ms |
| H2 | The operating system pauses the browser at random | Same 1,000 clicks with the recorder skipped | **Not supported.** No click reached 0.3 ms in 4 runs (the page-side helper had 0 of 3,000 earlier); the machine was idle |
| H3 | A particular click does extra work: the early send at 500 waiting records, serializing them inside a click | Read the code | **Rejected.** Sending is deferred with `setTimeout` ([activity-capture.js:37](../../frontend/ui/activity-capture.js#L37)), so it never runs inside a click |
| H4 | Click 0 is slow because the recorder's code is set up on first use | 5 warm-up clicks before measuring | **Supported.** Click 0 was fast in 4 of 4 runs |
| H5 | The big pause comes from firing 1,000 clicks with no idle time between them, which real use never does | Clicks one frame apart (about 16 ms) vs back to back, alternating, 6 runs each | **Not established.** Spaced: no click over 0.8 ms (click 0). Back to back in the same batch: no click over 0.7 ms either |

## What is and isn't known

- The slow click comes with a large memory clean-up (seen once with memory readings). The recorder
  makes about 1–2 KB of short-lived objects per click, against about 0.3–0.9 KB without it, so
  clean-ups come more often with it on.
- Not known: whether recording itself makes a clean-up *slow*, or only makes one land inside a click
  more often; and how often this happens with real, spaced-out clicks.
- The slow click's rate differed between batches: 4 of 8 runs of the real test, 1 of 10 runs of the
  probe. The probe ran Chrome with two extra options (`--enable-precise-memory-info`,
  `--js-flags=--expose-gc`), which may change when clean-up happens; untested.
- In the warm-up runs the per-click memory readings stayed flat while total memory grew by 3 MB; those
  readings were not used.

The probe was a temporary test file, deleted afterwards; its code is not kept.

## After reducing what the recorder allocates per click (2026-10-02, later)

Changes (plan C18): records stamped in place instead of copied, the current tab remembered instead
of looked up per click, tag names cached, already-clean text returned as is, no class-name arrays.
The page-side helper measured, with a throwaway probe (300 clicks per run, precise memory readings):
about 244 bytes per click before (typical 215–310), about 160 after (typical 128–233), 16 with
recording skipped; two after-runs were outliers (1,056 and 2,107 bytes), not traced.

The unchanged timing test, 20 runs requested, load average 0.3–2.2 (another session was running
its own tests at the same time): 7 runs did not start, because the test's web server port was
already in use, and are not counted. Of the 15 that ran, plus the full-suite run:

- **The large clean-up pause (500s–600s, 1.5–2.0 ms): in none.** Before: about half the runs.
- **Click 0: 0.5–1.2 ms; at or over 1 ms in 3 of 15** (1.0, 1.2, 1.0 ms), so the test still fails
  intermittently, now on the first click. Before: 0.5–0.8 ms in about 20 runs. Not established
  whether the change made the first click slower or the concurrent load did; the cheapest test is
  the same 20 runs on an otherwise idle machine.
- Average per click 16.6–38.0 µs; no other click reached 0.3 ms.

The earlier warm-up probe (H4) found click 0 fast after 5 warm-up clicks, consistent with the first
click on the review table being the first use of the recorder's code for describing elements inside
it (message id and column).
