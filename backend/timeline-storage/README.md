# timeline-storage

Concrete adapters implementing [`timeline-core`](../timeline-core/README.md)'s storage ports — see
that crate's README for what a "port" and an "adapter" mean here. Two families:

- **`memory`** — in-memory fakes (`Arc<Mutex<HashMap>>`), used by `timeline-api`'s own tests and by
  the local-dev server (`cargo run -p timeline-api`) to avoid any AWS/container dependency.
- **`s3`/`dynamo`** — the real AWS-backed adapters, for an eventual real deployment.

**Dependencies**: `timeline-core`, `async-trait`, `tokio`, `aws-sdk-s3`, `aws-sdk-dynamodb`,
`aws-smithy-types`, `uuid`, `serde_json`.

## Why this crate exists already, ahead of any real AWS deployment

Built during an earlier phase of this project's Rust migration (when the instruction was to build
the full V2 backend design), before this repo settled on staying container-free/AWS-free for
routine local testing (see the migration plan's §V2a). The real adapters compile against the
actual `aws-sdk-s3`/`aws-sdk-dynamodb` types, proving the port traits are shaped correctly for a
real implementation. Since the migration plan's §V2b they run against local stand-ins (`s3s-fs` for
S3, Amazon's DynamoDB Local for DynamoDB), but **they have never been run against real AWS.** The
in-memory adapters, built alongside them, are what every other crate's tests and the local-dev
server actually exercise.

## Test coverage — the honest split

Measured directly on 2026-10-01 (`cargo llvm-cov -p timeline-storage --summary-only`), not
estimated. Each storage interface has one **contract suite** in [tests/support/](tests/support/):
checks any correct implementation must pass, run against both the in-memory fake
([tests/contract_memory.rs](tests/contract_memory.rs)) and the real adapter, so a fake that drifts
from the real service fails a test.

| File | Line coverage | What's actually tested |
|---|---|---|
| [`s3.rs`](src/s3.rs) | 100% | `ObjectStore` contract against `s3s-fs`; presigned PUT/GET used by a plain HTTP client; tampered, expired and wrong-method URLs rejected; over-long presign and an unreachable server reported as `Backend`. |
| [`dynamo/conversations_table.rs`](src/dynamo/conversations_table.rs) | 98.41% | `UploadOutcomeStore` and `ConversationSummaryStore` contracts against DynamoDB Local; both row kinds in one table; malformed rows; missing table. Two lines unreached: the missing-`sk` and missing-`CONV#`-prefix branches in `list_for_user`, which no row DynamoDB can return should reach. |
| [`dynamo/message_flags_table.rs`](src/dynamo/message_flags_table.rs) | 99.44% | Message-flags contract (including the auto/user separation, read back through the real trait methods) against DynamoDB Local; a malformed sort key; missing table. One line unreached: the missing-`sk` branch, for the same reason. |
| `memory/*.rs` | 54–78% within this crate | Contract suites plus the original `memory_*.rs` tests. The unreached lines in each file are its `Resettable::reset`, which is exercised through `timeline-api`'s `POST /_dev/reset` tests, not from this crate. |

Not covered by the stand-ins: real S3's host-name bucket addressing and its `NoSuchBucket` error
(`s3s-fs` 0.17 doesn't check bucket existence on `GetObject`/`PutObject`). Those wait for the
real-AWS run in the migration plan's §V2.

## Design: what each adapter is adapting, and how

- **`InMemoryObjectStore`/`InMemoryUploadOutcomeStore`/`InMemoryConversationSummaryStore`/`InMemoryMessageFlagsStore`**
  — each wraps a `Mutex<HashMap<...>>`. `InMemoryObjectStore`'s presigned URLs are real, relative
  HTTP paths (`/_dev/local-storage/put|get/{key}`) that `timeline-api`'s `_dev`-only routes serve —
  not an inert placeholder string — so a real browser can actually `PUT`/`GET` against them in local
  dev (see the migration plan's §V2a).
- **`S3ObjectStore`** implements `ObjectStore` against a real `aws_sdk_s3::Client`, using
  `PresigningConfig::expires_in` for `presign_put`/`presign_get`.
- **`DynamoConversationsTable`** implements *both* `UploadOutcomeStore` and `ConversationSummaryStore`
  against one DynamoDB table (`Conversations`) — a standard single-table-design pattern,
  distinguishing an upload's own terminal-outcome row (written once, by `record_outcome` — see the
  migration plan's §V2a-revision) from the conversation summaries it eventually produces by
  sort-key prefix (`UPLOAD#<id>` vs. `CONV#<id>`).
- **`DynamoMessageFlagsStore`** implements the three flag traits against a separate `MessageFlags`
  table, with disjoint `auto_*`/`user_*` DynamoDB attribute names — the storage-level enforcement of
  the auto/user separation, verified by `auto_update_expression`/`user_update_expression`'s tests
  never referencing the other half's attributes.

## Class diagram

```mermaid
classDiagram
    class ObjectStore {
        <<trait, timeline-core>>
    }
    class UploadOutcomeStore {
        <<trait, timeline-core>>
    }
    class ConversationSummaryStore {
        <<trait, timeline-core>>
    }
    class MessageFlagsReader {
        <<trait, timeline-core>>
    }
    class AutoFlagWriter {
        <<trait, timeline-core>>
    }
    class UserFlagWriter {
        <<trait, timeline-core>>
    }

    class InMemoryObjectStore {
        -Mutex~HashMap~ objects
    }
    class InMemoryUploadOutcomeStore {
        -Mutex~HashMap~ outcomes
    }
    class InMemoryConversationSummaryStore {
        -Mutex~HashMap~ summaries
        +insert(user_id, summary)  "test-only sync helper"
    }
    class InMemoryMessageFlagsStore {
        -Mutex~HashMap~ records
    }
    class S3ObjectStore {
        -aws_sdk_s3::Client client
        -String bucket
    }
    class DynamoConversationsTable {
        -aws_sdk_dynamodb::Client client
        -String table_name
    }
    class DynamoMessageFlagsStore {
        -aws_sdk_dynamodb::Client client
        -String table_name
    }

    ObjectStore <|.. InMemoryObjectStore
    ObjectStore <|.. S3ObjectStore
    UploadOutcomeStore <|.. InMemoryUploadOutcomeStore
    UploadOutcomeStore <|.. DynamoConversationsTable
    ConversationSummaryStore <|.. InMemoryConversationSummaryStore
    ConversationSummaryStore <|.. DynamoConversationsTable
    MessageFlagsReader <|.. InMemoryMessageFlagsStore
    AutoFlagWriter <|.. InMemoryMessageFlagsStore
    UserFlagWriter <|.. InMemoryMessageFlagsStore
    MessageFlagsReader <|.. DynamoMessageFlagsStore
    AutoFlagWriter <|.. DynamoMessageFlagsStore
    UserFlagWriter <|.. DynamoMessageFlagsStore
```
