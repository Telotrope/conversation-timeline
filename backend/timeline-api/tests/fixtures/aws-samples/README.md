# Sample AWS events

Copied unchanged from the `aws_lambda_events` crate, version 0.16.1 (MIT license,
<https://github.com/awslabs/aws-lambda-rust-runtime>), `src/fixtures/`:

- `example-apigw-v2-request-jwt-authorizer.json`: an API Gateway HTTP API request with a JWT
  authorizer, in the format Lambda receives it.
- `example-s3-event.json`: an S3 "object created" notification.

The tests change only the fields this project's code reads (path, method, stage and
`Authorization` header; bucket name and object key) and leave the rest as the library ships it.

**These were not captured from this project's own deployment**, so they are not the verified
samples CLAUDE.md asks for. The migration plan's C24 and deployment check D8 replace them with
sanitized copies of real events once the stack is deployed.
