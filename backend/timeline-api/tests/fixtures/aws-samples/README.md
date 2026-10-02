# Sample AWS events

## `example-apigw-v2-request-jwt-authorizer.json`

An API Gateway HTTP API request with a JWT authorizer, in the format Lambda receives it. Copied
unchanged from the `aws_lambda_events` crate, version 0.16.1 (MIT license,
<https://github.com/awslabs/aws-lambda-rust-runtime>), `src/fixtures/`. The tests change only the
fields this project's code reads (path, method, stage and `Authorization` header).

**Not captured from this project's own deployment**, so it is not the verified sample CLAUDE.md
asks for (migration plan C24). It stays the library's because a real one carries a login token.

## `example-s3-event.json`

An S3 "object created" notification **captured from this project's dev stack on 2026-10-02**
(`infra/README.md` step 9; migration plan C24), then cleaned and reviewed by the user. Replaced
with placeholders: account number (`123456789012`), bucket name and ARN, stack ID, bucket-owner ID,
role ID in `userIdentity`, both AWS request IDs, `configurationId`, the user and upload IDs in the
key, and the eTag (now the checksum of an empty file). Kept as AWS sent it: field order, region,
event name and version (`2.6`), the `awsGeneratedTags` block, time, size, `sequencer`, stack name.

**One value AWS did not send:** `sourceIPAddress` is `127.0.0.1`, the address the library's sample
used. The logged line had it already replaced with `REDACTED`; the redaction test
(`tests/s3_event_logging.rs`) needs an address to remove, and checks for this one.

The tests change only the bucket name and object key.

## `example-destination-failure.json`

What Lambda sends an "on failure" destination after an asynchronous invocation's last retry fails
(plan `2026-10-02-upload-processing-failures.md` §2). **Written by Claude from memory of the
example invocation record in AWS's Lambda documentation ("Configuring destinations for
asynchronous invocation"), not copied from a source file and not captured from this project.**
Its `requestPayload` is `example-s3-event.json` above (the captured notification, since 2026-10-02); its other fields are still the documentation example. The plan's C5 replaces it with a real one
from the first failure after deployment.
