//! In-memory fakes for every storage port — used by `timeline-api`'s own
//! unit tests and for running the app locally without any AWS access at
//! all. Each fake enforces the same auto/user separation the real DynamoDB
//! adapter does, just with a `Mutex<HashMap<...>>` instead of a table.

pub mod conversations;
pub mod resettable;
pub mod message_flags;
pub mod object_store;
pub mod uploads;
