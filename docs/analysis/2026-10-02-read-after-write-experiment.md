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
