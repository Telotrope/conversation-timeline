# Read-after-write experiment on the deployed flags table

**Plan:** [2026-10-02-upload-processing-failures.md §0](../plans/2026-10-02-upload-processing-failures.md).
**Run:** 2026-10-02, from the Linux machine `dev`, against `timeline-message-flags-dev`
(us-east-1), with
[dynamo_read_after_write_experiment.rs](../../backend/timeline-storage/tests/dynamo_read_after_write_experiment.rs).

## Result: inconclusive

| Read kind | Reads | Missed the row just written | Write time, median / max | Read time, median / max |
|---|---|---|---|---|
| default (eventually consistent) | 300 | 0 | 44 ms / 619 ms | 41 ms / 79 ms |
| strongly consistent | 300 | 0 | 42 ms / 166 ms | 41 ms / 184 ms |

All 600 rows were deleted afterwards. Total run time was 81 s.

## What this does and doesn't show

- **It doesn't confirm the inferred cause.** No default read missed.
- **It doesn't refute it either, for two reasons:**
  1. **Timing.** Each request took about 40 ms from `dev` to DynamoDB and back. So each read reached
     DynamoDB roughly 20 ms or more after the write was acknowledged. From inside Lambda, in the
     same region, a request takes a few milliseconds (typical figure, not measured here), so the
     read arrives much sooner after the write. A replica that needs a few milliseconds to catch up
     would be missed from Lambda and not from here.
  2. **Sample size.** The deployed function hit 2 misses. Its third attempt then succeeded through
     all 538 reviews, but the log doesn't say how far attempts 1 and 2 got before failing. So the
     production miss rate is unknown. It could be about 1 in a few hundred. At 1 in 500, 300 reads
     would show no miss about 55% of the time.
- **Found along the way:** Amazon's Rust library reads `aws login` credentials only with the
  optional `credentials-login` feature of `aws-config` switched on. Without it, every request
  failed with a message naming that feature. This answers the migration plan's open item C38.

## What would give a clearer answer

- **More reads.** Thousands of default reads, sent several rows at a time, so the run stays short.
  This doesn't change the delay between each row's write and its read.
- **Reads from inside Lambda**, which matches production's timing. This needs a small temporary
  function, or a switch in the processing function.

## Second run: from inside Lambda (plan §0b)

**Run:** 2026-10-02, with the `read_after_write` Lambda
([timeline-experiments](../../backend/timeline-experiments/src/read_after_write.rs)), deployed as
the temporary stack `timeline-experiment-read-after-write`
([template](../../infra/experiments/read-after-write.yaml)): ARM, 512 MB, the same settings as the
processing function. Invoked three times with 4,000 rows each. The plan said one invocation; I
ran two more because one miss was too little to go on. The stack was deleted afterwards. A scan
of the flags table for `experiment#` keys found 0 rows left behind.

| Invocation | Default reads | Misses (row number) | Strongly consistent reads | Misses | Median write / read |
|---|---|---|---|---|---|
| 1 | 2,000 | 1 (row 1422) | 2,000 | 0 | 3.5 ms / 2.1 ms |
| 2 | 2,000 | 2 (rows 2206, 2960) | 2,000 | 0 | 3.7 ms / 2.0 ms |
| 3 | 2,000 | 0 | 2,000 | 0 | 4.7 ms / 2.3 ms |
| **Total** | **6,000** | **3** | **6,000** | **0** | |

Requests took about 2 to 4 ms from inside Lambda, against about 40 ms from `dev`. That's the
timing difference the first run's analysis predicted.

## What the second run shows

- **Default reads straight after a write do miss the new row**: 3 times in 6,000, about 1 in
  2,000. That's the inferred cause of the deployed failure, now observed directly.
- **Strongly consistent reads didn't miss in 6,000.** If they missed as often as default reads,
  0 in 6,000 would happen about 5% of the time (e^-3). So the difference is unlikely to be
  chance, though 6,000 reads can't prove they never miss. DynamoDB documents strongly consistent
  reads as reflecting every write acknowledged before them.
- **It fits the deployed failure.** At about 1 in 2,000, each of the upload's 538 review
  read-backs had that chance, so a whole attempt fails about 24% of the time
  (1 − e^(−538/2000)). Two failures in three attempts is unlucky, but within reason (about 13%).
- **What this doesn't show:** that no *other* cause contributed to the deployed failure, because
  the original log names no row (plan §1 fixes that).
