# timeline-api

The deployed backend: an `axum` app assembling routes over [`timeline-core`](../timeline-core/README.md)'s
domain logic and [`timeline-storage`](../timeline-storage/README.md)'s adapters, gated by
[`timeline-auth`](../timeline-auth/README.md)'s Cognito verification. Two entry points:

- `main.rs` runs the API either as a Lambda function (behind API Gateway) or as a local dev server
  (`cargo run -p timeline-api`), based on whether `AWS_LAMBDA_RUNTIME_API` is set.
- [`src/bin/process_upload.rs`](src/bin/process_upload.rs) is the upload-processing Lambda, started
  by S3 when a raw upload lands. Its handler is [`s3_trigger`](src/s3_trigger.rs) (migration plan
  §V2e, E2). Locally, the `_dev/local-storage` upload route calls the same processing directly.

`cargo lambda build --release --arm64 -p timeline-api` builds both, into
`target/lambda/timeline-api/` and `target/lambda/process_upload/`.

**Dependencies**: `timeline-core`, `timeline-storage`, `timeline-auth`, `axum`, `tokio`,
`lambda_http`, `lambda_runtime` and `aws_lambda_events` (the processing Lambda's S3 event),
`percent-encoding` (S3 event keys), `serde`/`serde_json`, `uuid`, `jsonwebtoken`, `tower-http` (CORS), `rsa`/`rand`/`base64`
(generating the dev-only signing keypair at runtime).

## Two routers, structurally separated

This crate builds **two different routers with two different state types**, not one router with
optional pieces:

- **`AppState`** ([src/state.rs](src/state.rs)) — the real, Cognito-gated API. Exposes each storage
  capability as its own `FromRef` implementation, so a route handler's function signature only ever
  names the one trait object it actually needs.
- **`DevState`** ([src/dev_state.rs](src/dev_state.rs)) — the `_dev`-namespaced local-testing
  surface.

`main.rs` merges `DevState`'s router into the running server **only** in the local-dev branch — the
Lambda branch is handed `build_router(app_state)` alone and never even constructs the merge. So the
`_dev` routes are structurally absent from anything that could run in production, not a convention
that could be forgotten. (`AutoFlagWriter` is in `AppState` since detection became a user-requested
route, `POST /detect`; see [src/state.rs](src/state.rs)'s module doc.)

## Routes

| Method & path | Handler | State |
|---|---|---|
| `POST /uploads` | [`routes::uploads::create_upload`](src/routes/uploads.rs) | `AppState` |
| `GET /uploads/{upload_id}` | [`routes::uploads::upload_status`](src/routes/uploads.rs): `processing`, `ready` or `failed` with a reason | `AppState` |
| `POST /detect` | [`routes::detect::detect`](src/routes/detect.rs) | `AppState` |
| `GET /conversations` | [`routes::conversations::list_conversations`](src/routes/conversations.rs) | `AppState` |
| `GET`/`PATCH /conversations/{id}/messages/{id}/flags` | [`routes::flags`](src/routes/flags.rs) | `AppState` (two *different* narrow states — see below) |
| `GET /export` | [`routes::export::export`](src/routes/export.rs) | `AppState` |
| `PUT`/`GET /_dev/local-storage/{put,get}/{*key}` | [`routes::dev_local_storage`](src/routes/dev_local_storage.rs) | `DevState`, local-dev only |
| `POST /_dev/login` | [`routes::dev_login::login`](src/routes/dev_login.rs) | none, local-dev only |

`GET`/`PATCH .../flags` are the concrete embodiment of the auto/user separation: the `PATCH`
handler is given a `UserFlagWriter` and no `AutoFlagWriter`, so its own source code has no way to
write an automatic flag — not "doesn't call it," genuinely not a parameter it could call. Flags live
on the message rows; a save finds its message among its conversation's rows by id, then recounts
the message's session (plan `docs/plans/2026-10-06-load-only-what-the-page-shows.md` §6).

**Requests in parts** (plan §8c). `GET /conversations`, `/sessions`, `/uploads`, `/messages`,
`/export`, `/conversations/{id}/files`, `/analyses/{name}`, `POST /detect` and
`PUT /uploads/{id}/metadata` each do as much as fits in the request's work limit (9 seconds on AWS;
`TIMELINE_WORK_BUDGET_STEPS` rows read, for the local test server) and answer with a `cursor` to
carry on from and the user's `data_version`. Every route that reads message rows does so through
[`message_query::find_messages`](src/message_query.rs).

**Flag handles** ([`flag_handles`](src/flag_handles.rs), migration plan §V2c). Each row of
`GET /messages` carries its message's `handle`, and each part of `GET /export` its messages'
`flag_handles`: one handle per user message, an HMAC-SHA256 signature over the user, conversation and message ids under a key only the
server holds. The exported `conversations.json` itself carries no handles. `PATCH .../flags` takes
`{"handle": ..., "caps"?, "critical"?, "angry"?}` and answers:

- **400** for an unknown field (named in the message), a missing `handle`, or nothing to change;
- **403** when the handle doesn't match the conversation and message in the address;
- **200** with the stored record otherwise.

So a save can only name a message the server actually sent.

**Two ways to build `AppState`.** Local dev (`main.rs`'s `build_local_state`) uses the in-memory
stores and the throwaway dev login keys. The Lambda uses
[`aws_state::build_aws_state`](src/aws_state.rs): the S3 and DynamoDB adapters, and a
`CognitoVerifier` loaded with the user pool's published keys, from settings read once by
[`aws_settings::AwsSettings`](src/aws_settings.rs). A missing setting, flag-handle key or key
download stops the Lambda at startup; nothing falls back to the local setup. See the migration
plan's §V2d. The key is generated at startup
locally; on Lambda it comes from `TIMELINE_FLAG_HANDLE_KEY` (filled from Secrets Manager by
[infra/template.yaml](../../infra/template.yaml)), and the Lambda refuses to start without it.

## Processing: composing timeline-core logic with the ports

[`processing::process_upload`](src/processing.rs) turns a raw upload into stored conversation
summaries and auto flags. It contains **no new domain logic** — it composes already-tested
`timeline-core` functions (`unwrap_uploaded_json`, the three flag detectors) with the storage ports,
generic over the trait objects so it's callable by either the S3-triggered processing Lambda
([`s3_trigger`](src/s3_trigger.rs)) or the local-dev `PUT /_dev/local-storage/...` handler (which calls it directly as a
stand-in for the real S3 event — see the migration plan's §V2a).

## Test coverage

163+ tests across the workspace exercise this crate through real HTTP requests
(`tower::ServiceExt::oneshot` against the actual `Router`, not a mock) — [tests/app.rs](tests/app.rs),
[tests/dev_routes.rs](tests/dev_routes.rs), [tests/export.rs](tests/export.rs),
[tests/processing.rs](tests/processing.rs), [tests/flag_saves.rs](tests/flag_saves.rs),
[tests/aws_state.rs](tests/aws_state.rs), [tests/upload_status.rs](tests/upload_status.rs),
[tests/s3_trigger.rs](tests/s3_trigger.rs) (the processing Lambda's handler against local S3 and
DynamoDB stand-ins), [tests/lambda_events.rs](tests/lambda_events.rs) (AWS-format requests through
`lambda_http`'s own conversion; see [tests/fixtures/aws-samples/](tests/fixtures/aws-samples/README.md)
for where the sample events come from). Additionally verified with a real, driven headless
browser via the top-level [`e2e/`](../../e2e/README.md) Playwright suite — uploading a real file
through the real local-dev server and confirming it renders in `timeline.html`.

Auth material for tests and local dev (`timeline_api::dev_only::DEV_KEYPAIR`) is a throwaway RSA
keypair **generated fresh at runtime**, never written to disk or checked into git — see
[src/dev_only.rs](src/dev_only.rs)'s module doc for why an earlier, checked-in-PEM-file design was
changed.

## Class diagram

```mermaid
classDiagram
    class AppState {
        +Arc~dyn ObjectStore~ object_store
        +Arc~dyn ConversationSummaryStore~ conversation_summary_store
        +Arc~dyn MessageReader~ message_reader
        +Arc~dyn UserFlagWriter~ user_flag_writer
        +Arc~dyn AutoFlagWriter~ auto_flag_writer
        +Arc~dyn SessionStore~ session_store
        +Arc~dyn UserRecordStore~ user_records
        +Arc~dyn AnalysisStore~ analysis_store
        +Arc~dyn UploadOutcomeStore~ upload_outcome_store
        +BudgetSetting budget
        +Arc~CognitoVerifier~ verifier
        +Arc~FlagHandleKey~ flag_handle_key
    }
    class DevState {
        +ProcessingStores processing
        +Vec~Resettable~ resettable
    }
    class AuthenticatedUser {
        +UserId
    }
    class ApiError {
        <<enum>>
        NotFound
        Store(StoreError)
        ObjectStore(ObjectStoreError)
        Internal(String)
    }
    class ProcessingError {
        <<enum>>
        RawObjectNotUtf8
        Format(FormatError)
        Store(StoreError)
        ObjectStore(ObjectStoreError)
    }
    class process_upload {
        <<function>>
        +process_upload(stores, user_id, upload_id) Result~(), ProcessingError~
    }
    class create_upload {
        <<handler, AppState>>
    }
    class list_conversations {
        <<handler, AppState>>
    }
    class get_flags {
        <<handler, AppState: MessageReader only>>
    }
    class patch_flags {
        <<handler, AppState: UserFlagWriter only>>
    }
    class export {
        <<handler, AppState>>
    }
    class put_object {
        <<handler, DevState>>
    }
    class get_object {
        <<handler, DevState>>
    }
    class login {
        <<handler, stateless>>
    }

    AppState --> AuthenticatedUser : extracted per-request
    create_upload --> AppState
    list_conversations --> AppState
    get_flags --> AppState
    patch_flags --> AppState
    export --> AppState
    put_object --> DevState
    get_object --> DevState
    put_object ..> process_upload : calls after storing bytes
    process_upload ..> ProcessingError
    create_upload ..> ApiError
    list_conversations ..> ApiError
    export ..> ApiError
```
