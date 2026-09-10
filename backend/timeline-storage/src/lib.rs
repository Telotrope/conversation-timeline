//! Concrete adapters for `timeline-core`'s storage ports -- see the
//! migration plan section 6.2 ("ports and adapters"). `memory` holds
//! in-memory fakes used by `timeline-api`'s own unit tests and local
//! development; `s3`/`dynamo` are the real AWS-backed implementations.

pub mod dynamo;
pub mod memory;
pub mod s3;
