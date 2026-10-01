//! Real DynamoDB-backed adapters. Tested against Amazon's DynamoDB Local in
//! `tests/dynamo_conversations_table.rs` and `tests/dynamo_message_flags.rs`
//! (the shared storage contracts, plus rows our code didn't write and a
//! missing table). Not yet run against real DynamoDB; the real-AWS run in
//! the migration plan's §V2 is still needed before V2 can be called done.

mod attributes;
pub mod conversations_table;
pub mod message_flags_table;
